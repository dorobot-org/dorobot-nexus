"""Real process and SSH protocol checks for durable training ownership."""
import base64
import copy
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
import agent
import client

SCRIPT='''import json,pathlib,subprocess,sys,time
root=pathlib.Path(sys.argv[1]);(root/'launched').write_text(str(__import__('os').getpid()))
if 'child' in sys.argv:
 subprocess.Popen([sys.executable,'-c',"import pathlib,sys,time;p=pathlib.Path(sys.argv[1]);[(p.write_text(str(i)),time.sleep(.03)) for i in range(10000)]",str(root/'child-tick')])
for i in range(10000):
 print(json.dumps(dict(event='sample',step=i+1,reward=.5,fall_rate=0.,steps_per_sec=1.,episode_len=1.,path=str(root/'policy.pt'))),flush=True)
 time.sleep(.03)
'''

def until(root,token,predicate,timeout=8):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        state=agent.status(root,token)
        if predicate(state):return state
        time.sleep(.04)
    raise AssertionError(state)

class Supervisor(unittest.TestCase):
    def setUp(self):self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)/'run-id';self.pid=None
    def tearDown(self):
        if self.pid:
            try:os.killpg(self.pid,signal.SIGCONT);os.killpg(self.pid,signal.SIGKILL)
            except ProcessLookupError:pass
        self.temp.cleanup()
    def prepare(self,script=SCRIPT):
        self.root.mkdir();spec=dict(version=1,token='unit-token',command=[sys.executable,str(self.root/'worker.py'),str(self.root),'child'],cwd=str(self.root),env={},inputs={'worker.py':agent.digest(script.encode())})
        req=dict(op='prepare',root=str(self.root),spec=spec,files={'worker.py':dict(data=base64.b64encode(script.encode()).decode(),sha256=agent.digest(script.encode()))})
        agent.rpc(req);return spec
    def test_detached_worker_pause_resume_stop_and_idempotent_start(self):
        spec=self.prepare();agent.start(self.root,spec['token']);state=until(self.root,spec['token'],lambda s:s['state']=='Running');self.pid=state['worker_pid'];time.sleep(.2)
        self.assertEqual(agent.start(self.root,spec['token'])['worker_pid'],self.pid)
        self.assertEqual(state['launches'],1)
        agent.control(self.root,spec['token'],'pause','p1');until(self.root,spec['token'],lambda s:s['state']=='Paused')
        time.sleep(.1);size=(self.root/'job-events.jsonl').stat().st_size;child=(self.root/'child-tick').read_bytes();time.sleep(.15)
        self.assertEqual((self.root/'job-events.jsonl').stat().st_size,size);self.assertEqual((self.root/'child-tick').read_bytes(),child)
        agent.control(self.root,spec['token'],'resume','r1');until(self.root,spec['token'],lambda s:s['state']=='Running');time.sleep(.1);self.assertGreater((self.root/'job-events.jsonl').stat().st_size,size)
        agent.control(self.root,spec['token'],'stop','s1');until(self.root,spec['token'],lambda s:s['state']=='Stopped');self.assertEqual(agent.start(self.root,spec['token'])['state'],'Stopped')
    def test_changed_input_and_conflicting_recipe_never_launch(self):
        spec=self.prepare();(self.root/'worker.py').write_text('changed')
        agent.start(self.root,spec['token']);state=until(self.root,spec['token'],lambda s:s['state']=='Failed');self.assertIn('Captured input changed',state['error']);self.assertFalse((self.root/'launched').exists())
        with self.assertRaises(ValueError):agent.status(self.root,'other-token')
        altered=copy.deepcopy(spec);altered['command']=['unrelated'];
        with self.assertRaises(ValueError):agent.prepare(dict(root=str(self.root),spec=altered))
    def test_supervisor_loss_is_unknown_and_cannot_duplicate_worker(self):
        spec=self.prepare();agent.start(self.root,spec['token']);state=until(self.root,spec['token'],lambda s:s['state']=='Running');self.pid=state['worker_pid'];until(self.root,spec['token'],lambda s:(self.root/'launched').exists());os.kill(state['supervisor_pid'],signal.SIGKILL)
        until(self.root,spec['token'],lambda s:s['state']=='Unknown')
        self.assertEqual(agent.start(self.root,spec['token'])['state'],'Unknown');self.assertTrue((self.root/'launched').exists())
        with self.assertRaises(ValueError):agent.control(self.root,spec['token'],'stop','unsafe')
    def test_nonzero_exit_is_failed_and_queued_cancel_never_launches(self):
        spec=self.prepare('raise RuntimeError("actual worker failure")\n');state=agent.start(self.root,spec['token']);self.assertEqual(until(self.root,spec['token'],lambda s:s['state']=='Failed')['returncode'],1)
        other=self.root.parent/'queued';spec['command']=[sys.executable,'-c','pass'];spec['inputs']={}
        agent.prepare(dict(root=str(other),spec=spec));agent.control(other,spec['token'],'stop','cancel');self.assertEqual(agent.start(other,spec['token'])['state'],'Stopped')

    def test_changed_output_cannot_replace_a_complete_local_checkpoint(self):
        spec=self.prepare();checkpoint=self.root/'policy.pt';checkpoint.write_bytes(b'x'*(4*1024*1024+10))
        download=self.root.parent/'download';download.mkdir();(download/'.job').mkdir();(download/'policy.pt').write_bytes(b'previous complete checkpoint')
        doc=dict(id=self.root.name,local_root=str(download),source_root='/local/repo',spec=spec,settings=dict(transport='ssh',remote_root=str(self.root.parent),remote_repo='/remote/repo'))
        def transfer(config,req):
            if req['op']=='file' and req['path']=='policy.pt' and req['offset']>0:
                with checkpoint.open('r+b') as f:f.write(b'changed')
            return agent.rpc(req)
        with patch.object(client,'invoke',side_effect=transfer),self.assertRaisesRegex(ValueError,'changed during transfer'):
            client.sync(doc,final=True)
        self.assertEqual((download/'policy.pt').read_bytes(),b'previous complete checkpoint')
        self.assertFalse((download/'.job/fetched.json').exists())

    def test_submit_rejects_changed_input_with_durable_failure(self):
        self.root.mkdir();data=b'print("must not execute")\n';(self.root/'worker.py').write_bytes(data)
        spec=dict(version=1,token='changed-submit',command=[sys.executable,str(self.root/'worker.py')],cwd=str(self.root),env={},inputs={'worker.py':agent.digest(data)},source_checks={})
        doc=dict(id=self.root.name,local_root=str(self.root),source_root=str(self.root.parent),spec=spec,settings=dict(transport='local'))
        (self.root/'worker.py').write_text('print("changed")\n')
        client.submit(doc)
        state=until(self.root,spec['token'],lambda s:s['state']=='Failed')
        self.assertIn('Captured input changed',state['error']);self.assertNotIn('worker_pid',state)
        (self.root/'worker.py').unlink()
        self.assertEqual(client.submit(doc)['state'],'Failed')

class SshProtocol(unittest.TestCase):
    @unittest.skipUnless(os.environ.get('NEXUS_TEST_SSH')=='1','explicit isolated SSH fixture required')
    def test_verified_ssh_transfer_and_host_failure(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)/'local'/'remote-run';root.mkdir(parents=True);remote=Path(temp)/'remote'
            script="import json,pathlib,sys,os;root=pathlib.Path(os.environ['NEXUS_RUNS_DIR'])/pathlib.Path(sys.argv[1]).name;(root/'policy.pt').write_bytes(b'checkpoint-bytes');(root/'result.json').write_text(json.dumps(dict(checkpoint=str(root/'policy.pt'))));print(json.dumps(dict(event='sample',step=32,reward=1.,fall_rate=0.,steps_per_sec=1.,episode_len=1.)))"
            (root/'worker.py').write_text(script)
            settings=dict(transport='ssh',host=os.environ.get('USER','user')+'@127.0.0.1',port=22489,identity='/tmp/nexus-job-ssh/client_key',known_hosts='/tmp/nexus-job-ssh/known_hosts',remote_root=str(remote),remote_repo=str(Path(temp)/'remote-repo'),python=sys.executable,studio_binary=sys.executable)
            spec=dict(version=1,token='ssh-token',command=[sys.executable,str(root/'worker.py'),str(root)],cwd=str(root),env={'NEXUS_RUNS_DIR':str(root.parent)},inputs={'worker.py':agent.digest(script.encode())},source_checks={})
            doc=dict(id=root.name,settings=settings,local_root=str(root),source_root=str(Path(temp)/'local-repo'),spec=spec)
            bad=copy.deepcopy(doc);bad['settings']['known_hosts']=str(Path(temp)/'empty-known-hosts');Path(bad['settings']['known_hosts']).touch()
            with self.assertRaises(RuntimeError):client.submit(bad)
            self.assertFalse((remote/root.name).exists())
            result=client.submit(doc)
            deadline=time.monotonic()+10
            while result['state'] not in agent.TERMINAL and time.monotonic()<deadline:
                result=client.invoke(settings,dict(op='status',root=str(remote/root.name),token='ssh-token'));time.sleep(.1)
            self.assertEqual(result['state'],'Completed',result)
            (root/'.job').mkdir(exist_ok=True);client.sync(doc)
            self.assertEqual((root/'policy.pt').read_bytes(),b'checkpoint-bytes')
            self.assertEqual(json.loads((root/'result.json').read_text())['checkpoint'],str(root/'policy.pt'))
            self.assertEqual(client.submit(doc)['worker_pid'],result['worker_pid'])
            self.assertEqual(json.loads((root/'job-events.jsonl').read_text())['step'],32)

if __name__=='__main__':unittest.main()
