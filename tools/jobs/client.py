#!/usr/bin/env python3
"""Local/SSH job transport. SSH retains host verification and existing credentials."""
import base64
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time
from agent import atomic,digest,local

# The authenticated SSH account installs only the supplied, versioned agent.
BOOTSTRAP="""import base64,json,pathlib,subprocess,sys
v=json.load(sys.stdin);data=base64.b64decode(v['agent']);import hashlib
if hashlib.sha256(data).hexdigest()!=v['agent_sha256']:raise ValueError('agent checksum')
p=pathlib.Path(v['agent_path']);p.parent.mkdir(parents=True,exist_ok=True)
if p.exists():
 if p.read_bytes()!=data:raise ValueError('agent collision')
else:
 with p.open('xb') as f:f.write(data)
s=subprocess.run([sys.executable,str(p)],input=json.dumps(v['request']),text=True,capture_output=True)
sys.stdout.write(s.stdout);sys.stderr.write(s.stderr);sys.exit(s.returncode)
"""

def invoke(config,request):
    if config['transport']=='local':
        from agent import rpc
        return rpc(request)
    host=config['host']
    if not host or host.startswith('-') or any(c.isspace() or ord(c)<32 for c in host):raise ValueError('Invalid SSH host')
    agent=Path(__file__).with_name('agent.py').read_bytes();checksum=digest(agent)
    remote_agent=str(Path(config['remote_root'])/'.nexus-tools'/('agent-'+checksum+'.py'))
    cmd=['ssh','-T','-oBatchMode=yes','-oStrictHostKeyChecking=yes','-oConnectTimeout=8','-p',str(config['port'])]
    if config.get('identity'):cmd+=['-i',config['identity']]
    if config.get('known_hosts'):cmd+=['-oUserKnownHostsFile='+config['known_hosts']]
    cmd+=[host,shlex.join([config['python'],'-c',BOOTSTRAP])]
    packet=dict(agent=base64.b64encode(agent).decode(),agent_sha256=checksum,agent_path=remote_agent,request=request)
    result=subprocess.run(cmd,input=json.dumps(packet),text=True,capture_output=True,timeout=30)
    if result.returncode:raise RuntimeError('SSH job transport: '+result.stderr[-1500:])
    response=json.loads(result.stdout)
    if not response['ok']:raise RuntimeError(response['error'])
    return response['result']

def rewrite(value,mappings):
    if isinstance(value,str):
        for a,b in mappings:
            # Profiles contain both complete paths and paths inside flag strings.
            value=value.replace(a,b)
        return value
    if isinstance(value,list):return [rewrite(v,mappings) for v in value]
    if isinstance(value,dict):return {rewrite(k,mappings):rewrite(v,mappings) for k,v in value.items()}
    return value

def remote_root(doc):
    return str(Path(doc['settings']['remote_root'])/doc['id']) if doc['settings']['transport']=='ssh' else doc['local_root']

def submit(doc,start_job=True):
    config=doc['settings'];root=Path(doc['local_root']);destination=remote_root(doc)
    existing=invoke(config,dict(op='probe',root=destination,token=doc['spec']['token']))
    if existing is not None:
        # An earlier submission may have succeeded before the connection broke.
        # Observe its identity before touching the submitting machine's inputs.
        return invoke(config,dict(op='start' if start_job else 'control',root=destination,token=doc['spec']['token'],action='stop',request_id='cancel-before-start'))
    mappings=[]
    if config['transport']=='ssh':mappings=[(str(root),destination),(doc['source_root'],config['remote_repo'])]
    spec=rewrite(doc['spec'],mappings)
    if config['transport']=='ssh':
        spec['command'][0]=config['studio_binary']
        profile=doc.get('profile_python')
        if profile:
            # The worker JSON below also receives the configured remote interpreter.
            mappings.insert(0,(profile,config['python']))
            spec=rewrite(doc['spec'],mappings);spec['command'][0]=config['studio_binary']
        # This points to the parent of the run, which is not covered by the
        # run-directory mapping. Never inherit the submitting machine's storage.
        spec.setdefault('env',{})['NEXUS_RUNS_DIR']=str(Path(destination).parent)
    files={};inputs={}
    for name in doc['spec']['inputs']:
        try:data=local(root,name).read_bytes()
        except OSError:
            spec['input_error']='Captured input is unavailable: '+name
            continue
        if digest(data)!=doc['spec']['inputs'][name]:
            spec['input_error']='Captured input changed: '+name
        elif name.endswith('.json') and mappings:
            data=(json.dumps(rewrite(json.loads(data),mappings),allow_nan=False)+'\n').encode()
        files[name]=dict(data=base64.b64encode(data).decode(),sha256=digest(data));inputs[name]=digest(data)
    spec['inputs']=inputs
    invoke(config,dict(op='prepare',root=destination,spec=spec,files=files))
    return invoke(config,dict(op='start' if start_job else 'control',root=destination,token=doc['spec']['token'],action='stop',request_id='cancel-before-start'))

def sync(doc,final=False):
    config=doc['settings'];destination=remote_root(doc)
    if config['transport']=='local':return
    token=doc['spec']['token'];root=Path(doc['local_root']);cache=root/'.job/fetched.json'
    known=json.loads(cache.read_text()) if cache.exists() else {}
    inventory=invoke(config,dict(op='inventory',root=destination,token=token))
    for name,stamp in inventory.items():
        if known.get(name)==stamp:continue
        path=local(root,name);path.parent.mkdir(parents=True,exist_ok=True)
        temp=path.with_name('.'+path.name+'.download')
        with temp.open('wb') as f:
            offset=0
            while offset<stamp[0]:
                packet=invoke(config,dict(op='file',root=destination,token=token,path=name,offset=offset,limit=min(4*1024*1024,stamp[0]-offset),stamp=stamp))
                data=base64.b64decode(packet['data'],validate=True)
                if digest(data)!=packet['sha256'] or packet.get('stamp')!=stamp or not data:raise ValueError('Remote output changed during transfer')
                f.write(data);offset+=len(data)
            f.flush();os.fsync(f.fileno())
        if path.suffix in ('.json','.jsonl'):
            raw=temp.read_text();lines=raw.splitlines(keepends=True);translated=[]
            for line in lines:
                try:translated.append(json.dumps(rewrite(json.loads(line),[(destination,str(root)),(config['remote_repo'],doc['source_root'])]),allow_nan=False)+'\n')
                except json.JSONDecodeError:
                    # Pretty JSON is handled as one value; partial live JSON is retried.
                    try:translated=[json.dumps(rewrite(json.loads(raw),[(destination,str(root)),(config['remote_repo'],doc['source_root'])]),allow_nan=False)+'\n'];break
                    except json.JSONDecodeError:translated=None;break
            if translated is None:
                temp.unlink()
                if final:raise ValueError('Finished job has invalid JSON output: '+name)
                continue
            temp.write_text(''.join(translated))
        temp.replace(path);known[name]=stamp
    atomic(cache,known)

def main():
    request=json.load(sys.stdin);doc=request['job'];op=request['op']
    if op in ('submit','cancel'):result=submit(doc,op=='submit')
    else:
        args=dict(op=op,root=remote_root(doc),token=doc['spec']['token'])
        for k in ('action','request_id'):
            if k in request:args[k]=request[k]
        result=invoke(doc['settings'],args)
    if op in ('status','submit'):sync(doc,final=result['state'] in ('Completed','Failed','Stopped'))
    print(json.dumps(dict(ok=True,result=result),allow_nan=False))
if __name__=='__main__':
    try:main()
    except Exception as e:print(json.dumps(dict(ok=False,error=str(e))))
