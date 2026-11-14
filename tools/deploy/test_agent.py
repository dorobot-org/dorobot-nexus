"""Real HTTP/Unix socket acceptance; no physical robot is contacted."""
import io
import json
from pathlib import Path
import secrets
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import zipfile
from types import SimpleNamespace

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/training'))
from agent import Agent,Handler,Server,atomic,pack,unpack
from backends import Rpc
from client import Client


class AgentTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory(prefix='nexus-agent-test-');cls.path=Path(cls.temp.name)
        import torch
        from test_generic_task import URDF,fixture
        device='mps' if torch.backends.mps.is_available() else 'cuda' if torch.cuda.is_available() else None
        if device is None:raise unittest.SkipTest('Deployment acceptance trains real weights and requires an MPS or CUDA GPU')
        model=cls.path/'fixture.urdf';model.write_text(URDF);config=fixture(model)
        config['robot']='Portable actuator fixture';config['ppo']['device']=device;config['simulation']['episode_seconds']=20.
        task=cls.path/'task.json';task.write_text(json.dumps(config));cls.fixture=cls.path/'run'
        result=subprocess.run([sys.executable,str(ROOT/'tools/training/generic_worker.py'),'--task-config',str(task),'--run-dir',str(cls.fixture)],capture_output=True,text=True,timeout=120)
        if result.returncode:raise AssertionError(result.stdout[-1000:]+result.stderr[-2000:])
        from export_checkpoint import export
        cls.bundle=cls.path/'bundle'
        export(SimpleNamespace(checkpoint=cls.fixture/'policy.pt',output=cls.bundle,robot='Portable actuator fixture',studio_root=ROOT,recipe_file=None))
        cls.payload=pack(cls.bundle)
    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()
    def setUp(self):
        self.dir=self.path/secrets.token_hex(5)
        self.args=SimpleNamespace(state=self.dir,adapter='simulator',robot='Portable actuator fixture',root=ROOT,watchdog=.4,command_limits=[.2,.1,.5],hardware_config=None)
        self.agent=Agent(self.args);self.server=Server(('127.0.0.1',0),Handler);self.server.agent=self.agent;self.server.token=secrets.token_hex(32)
        threading.Thread(target=self.server.serve_forever,kwargs={'poll_interval':.02},daemon=True).start()
        threading.Thread(target=self.agent.loop,daemon=True).start()
        self.client=Client(f'http://127.0.0.1:{self.server.server_port}',self.server.token)
        self.client.command('attach')
    def tearDown(self):
        with self.agent.lock:self.agent.stop('test teardown');self.agent.shutdown=True
        self.server.shutdown();self.server.server_close()
    def prepare(self):self.client.call('/bundle',self.payload);self.client.command('dry-run')
    def test_authenticated_state_machine_command_limits_and_watchdog(self):
        wrong=Client(self.client.url,'invalid-token')
        with self.assertRaises(RuntimeError):wrong.call('/status')
        with self.assertRaises(RuntimeError):self.client.command('arm')
        self.prepare();s=self.client.command('arm');self.assertTrue(s['simulation']);self.assertEqual(s['stage'],0)
        with self.assertRaises(RuntimeError):self.client.command('command',velocity=[.1,0,0])
        with self.assertRaises(RuntimeError):self.client.command('advance')
        with self.assertRaises(RuntimeError):self.client.command('command',velocity=[float('nan'),0,0])
        deadline=time.monotonic()+3.2
        while time.monotonic()<deadline:self.client.command('heartbeat');time.sleep(.1)
        s=self.client.command('advance');self.assertEqual(s['stage'],1)
        self.client.command('command',velocity=[.04,0,0]);time.sleep(.1)
        s=self.client.call('/status');self.assertGreater(s['steps'],0);self.assertEqual(s['command'],[.04,0,0]);self.assertTrue(s['telemetry']['joints'])
        time.sleep(.6)
        self.assertEqual(self.client.call('/status')['state'],'stopped')
        events=[json.loads(l) for l in (self.dir/'events.jsonl').read_text().splitlines()]
        self.assertTrue(any(e['event']=='stop' and e['detail']=='heartbeat expired' for e in events))
    def test_replay_rejected_and_rollback_survives_agent_restart(self):
        self.prepare();first=self.client.command('arm')['candidate']['id'];self.client.command('stop')
        other=self.path/'bundle-other'
        if not other.exists():
            from export_checkpoint import export
            export(SimpleNamespace(checkpoint=self.fixture/'training/model_0.pt',output=other,robot='Portable actuator fixture',studio_root=ROOT,recipe_file=None))
        self.client.call('/bundle',pack(other));self.client.command('dry-run');self.client.command('arm');self.client.command('stop')
        with self.assertRaises(RuntimeError):self.client.call('/command',json.dumps(dict(op='heartbeat',owner=self.client.owner,sequence=1)).encode())
        self.assertEqual(self.client.command('rollback')['candidate']['id'],first)
        self.assertEqual(self.client.call('/status')['state'],'prepared')
        recovered=Agent(self.args)
        self.assertEqual(recovered.state,'connected');self.assertIsNone(recovered.candidate);self.assertTrue(recovered.last_deployed)
    def test_untrusted_bundles_and_failed_prepare_invalidate_arming(self):
        for name in ['../escape','/escape','foo\\bar']:
            b=io.BytesIO()
            with zipfile.ZipFile(b,'w') as z:z.writestr(name,b'unsafe')
            with self.assertRaises(ValueError):unpack(b.getvalue(),self.dir/'unsafe')
        self.prepare()
        wrong=self.path/'wrong-bundle'
        if not wrong.exists():
            import shutil
            shutil.copytree(self.bundle,wrong)
            p=json.loads((wrong/'provenance.json').read_text());p['observations']+=1;atomic(wrong/'provenance.json',p)
        with self.assertRaises(RuntimeError):self.client.call('/bundle',pack(wrong))
        with self.assertRaises(RuntimeError):self.client.command('arm')
        self.assertIsNone(self.client.call('/status')['candidate'])
    def test_bridge_exports_learned_policy_and_stops_when_gui_lease_ends(self):
        session=self.dir/'desktop';session.mkdir();token=session/'token';token.write_text(self.server.token)
        atomic(session/'config.json',dict(url=self.client.url,token_file=str(token),robot=self.args.robot));(session/'heartbeat').touch()
        proc=subprocess.Popen([sys.executable,str(ROOT/'tools/deploy/client.py'),'--session',str(session)],stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
        stop=threading.Event()
        def alive():
            while not stop.wait(.1):(session/'heartbeat').touch()
        threading.Thread(target=alive,daemon=True).start()
        def wait_for(fn):
            end=time.monotonic()+20
            while time.monotonic()<end:
                if (session/'status.json').exists():
                    s=json.loads((session/'status.json').read_text())
                    if fn(s):return s
                time.sleep(.05)
            raise AssertionError(s)
        try:
            wait_for(lambda s:s['state']=='connected')
            for i,op in enumerate(['prepare','dry-run','arm'],1):
                request=dict(id=i,op=op)
                if op=='prepare':request['checkpoint']=str(self.fixture/'policy.pt')
                atomic(session/f'command-{i:08}.json',request)
                s=wait_for(lambda s:s['ack']==i)
                self.assertIsNone(s['error'],s)
            self.assertEqual(s['agent']['state'],'armed')
            stop.set();proc.wait(timeout=5)
            self.assertEqual(self.client.call('/status')['state'],'stopped')
        finally:
            stop.set()
            if proc.poll() is None:proc.kill()
            proc.wait()
            stderr=proc.stderr.read().decode();proc.stderr.close();self.assertNotIn('Traceback',stderr)


class RpcTest(unittest.TestCase):
    def test_official_robotd_framing_notifications_and_refusal(self):
        with tempfile.TemporaryDirectory(prefix='robotd-rpc-test-') as directory:
            received=[]
            class Handler(socketserver.StreamRequestHandler):
                def handle(self):
                    for raw in self.rfile:
                        request=json.loads(raw);received.append(request)
                        if 'id' not in request:continue
                        if request['method']=='robot.enable':result=dict(accepted=False,reason='test refusal')
                        elif request['method']=='hello':result=dict(api_version=28,daemon_version=None,revision=None)
                        else:result=dict(accepted=True)
                        self.wfile.write((json.dumps(dict(jsonrpc='2.0',id=request['id'],result=result))+'\n').encode());self.wfile.flush()
            server=socketserver.ThreadingUnixStreamServer(str(Path(directory)/'robotd.sock'),Handler);server.daemon_threads=True
            threading.Thread(target=server.serve_forever,kwargs={'poll_interval':.02},daemon=True).start()
            rpc=Rpc(Path(directory)/'robotd.sock')
            try:
                rpc.call('robot.move',dict(vx=.1,vy=0,vyaw=0),notify=True)
                with self.assertRaisesRegex(ValueError,'test refusal'):rpc.call('robot.enable',dict(on=True))
                self.assertNotIn('id',next(r for r in received if r['method']=='robot.move'))
                self.assertEqual(received[0]['params'],{'api_version':28})
            finally:rpc.reader.close();rpc.socket.close();server.shutdown();server.server_close()


if __name__=='__main__':unittest.main()
