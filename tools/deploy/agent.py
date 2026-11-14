#!/usr/bin/env python3
"""Nexus robot agent. Loopback HTTP; use an authenticated SSH tunnel for remote targets."""
from __future__ import annotations
import argparse
import hashlib
import hmac
import io
import json
import math
import os
from pathlib import Path
import secrets
import signal
import socketserver
import stat
import sys
import threading
import time
import zipfile
from http.server import BaseHTTPRequestHandler, HTTPServer

LIMIT = 256 * 1024 * 1024
VERSION = 1
STAGES = (0., .25, .5, 1.)


def atomic(path, value):
    tmp = path.with_suffix(path.suffix+'.tmp')
    tmp.write_text(json.dumps(value, indent=2, allow_nan=False))
    os.replace(tmp, path)


def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()


def unpack(data, folder):
    """New directory only; no links, duplicate members or traversal; every file is hashed."""
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        infos = archive.infolist()
        names = [i.filename for i in infos]
        if len(infos) > 2048 or len(set(names)) != len(names) or sum(i.file_size for i in infos) > LIMIT:
            raise ValueError('Bundle is too large or has duplicate members')
        for i in infos:
            p = Path(i.filename)
            if not i.filename or p.is_absolute() or '..' in p.parts or '\\' in i.filename or stat.S_ISLNK(i.external_attr >> 16):
                raise ValueError('Unsafe bundle member')
        manifest = json.loads(archive.read('deployment-manifest.json'))
        expected = manifest['files']
        if set(expected) != set(names)-{'deployment-manifest.json'}:
            raise ValueError('Bundle file list disagrees with manifest')
        for name, sha in expected.items():
            if hashlib.sha256(archive.read(name)).hexdigest() != sha:
                raise ValueError(f'Bundle digest changed: {name}')
        folder.mkdir(exist_ok=False)
        try:
            for name in names:
                p = folder/name
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_bytes(archive.read(name))
        except BaseException:
            import shutil
            shutil.rmtree(folder)
            raise
    return manifest


def pack(folder):
    files = {}
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, 'w', zipfile.ZIP_DEFLATED) as archive:
        for p in sorted(folder.rglob('*')):
            if p.is_symlink(): raise ValueError('Linked policy bundles cannot be deployed')
            if p.is_file() and p.name != 'deployment-manifest.json':
                if p.stat().st_size > LIMIT: raise ValueError('Oversized bundle file')
                name = str(p.relative_to(folder))
                files[name] = digest(p)
                archive.write(p, name)
        archive.writestr('deployment-manifest.json', json.dumps(dict(version=1, files=files)))
    if len(stream.getvalue()) > LIMIT: raise ValueError('Oversized policy bundle')
    return stream.getvalue()


def verify_bundle(folder):
    expected=json.loads((folder/'deployment-manifest.json').read_text())['files']
    actual={str(p.relative_to(folder)) for p in folder.rglob('*') if p.is_file() and p.name!='deployment-manifest.json'}
    if actual!=set(expected):raise ValueError('Retained bundle file list changed')
    for name,sha in expected.items():
        path=folder/name
        if Path(name).is_absolute() or '..' in Path(name).parts or path.is_symlink() or digest(path)!=sha:
            raise ValueError('Retained bundle digest changed: '+name)


class Agent:
    def __init__(self, args, backend=None):
        self.args = args
        args.state.mkdir(parents=True, exist_ok=True)
        self.lock = threading.RLock()
        self.backend = backend
        self.connected = False
        self.connection_attempted = False
        self.connection_info = None
        self.owner, self.sequence = None, -1
        self.heartbeat = 0.
        self.state = 'connected'
        self.stage, self.started, self.steps = 0, 0., 0
        self.command = [0., 0., 0.]
        self.candidate = None
        self.dry_run = None
        self.telemetry = None
        self.error = None
        self.shutdown = False
        self.previous = None
        self.history = []
        saved = args.state/'state.json'
        if saved.is_file():
            prior = json.loads(saved.read_text())
            self.previous = prior.get('last_deployed')
            self.history = prior.get('history', [])
        self.last_deployed = self.previous
        # A process restart never resumes motion or a policy automatically.
        self.record('agent_start', dict(adapter=args.adapter, recovered_previous=self.previous))

    def record(self, event, detail=None):
        with (self.args.state/'events.jsonl').open('a') as file:
            file.write(json.dumps(dict(time=time.time(), event=event, state=self.state, detail=detail), allow_nan=False)+'\n')
        atomic(self.args.state/'state.json', self.status())

    def status(self):
        return dict(version=VERSION, robot=self.args.robot, adapter=self.args.adapter,
                    simulation=self.args.adapter == 'simulator', hardware_validated=False,
                    state=self.state, stage=self.stage, command_fraction=STAGES[self.stage],
                    command=self.command, steps=self.steps, dwell_seconds=max(0., time.monotonic()-self.started) if self.state == 'armed' else 0.,
                    candidate=self.candidate, dry_run=self.dry_run, telemetry=self.telemetry,
                    error=self.error, last_deployed=self.last_deployed, previous=self.previous,
                    history=self.history[-20:], watchdog_seconds=self.args.watchdog,
                    next_stage_after_seconds=3.)

    def make_backend(self):
        if self.backend is not None: return
        from backends import make_backend
        self.backend = make_backend(self.args)

    def stop(self, reason):
        self.command = [0., 0., 0.]
        try:
            if self.backend and self.state in ('armed','arming','stop_unconfirmed'): self.backend.stop()
        except Exception as e:
            self.error = f'Stop acknowledgement failed: {e}'
            self.state = 'stop_unconfirmed'
        else:
            self.state = 'stopped'
        self.record('stop', reason)

    def upload(self, data):
        with self.lock:
            if self.state in ('armed', 'stop_unconfirmed'):
                raise ValueError('Stop and confirm the target before changing policy')
            identity = hashlib.sha256(data).hexdigest()
            folder = self.args.state/'bundles'/identity
            folder.parent.mkdir(exist_ok=True)
            if not folder.exists(): unpack(data, folder)
            verify_bundle(folder)
            source = json.loads((folder/'provenance.json').read_text())
            if source.get('robot') != self.args.robot or digest(folder/'policy.onnx') != source.get('onnx_sha256'):
                raise ValueError('Policy robot or ONNX digest is incompatible with this target')
            self.make_backend()
            self.state, self.candidate, self.dry_run = 'connected', None, None
            self.backend.prepare(folder)
            self.candidate = dict(id=identity, checkpoint_sha256=source['checkpoint_sha256'], policy_sha256=source['onnx_sha256'],
                                  observations=source['observations'], actions=source['actions'])
            self.state, self.dry_run, self.error = 'prepared', None, None
            self.record('prepare', self.candidate)
            return self.status()

    def request(self, request):
        with self.lock:
            op = request.get('op')
            if op == 'attach':
                if self.state in ('armed', 'stop_unconfirmed'): raise ValueError('Target is active; stop it before attaching another session')
                self.owner, self.sequence = secrets.token_hex(16), -1
                self.make_backend()
                if not self.connected:
                    if self.connection_attempted: raise ValueError('Target connection failed; restart the agent after correcting its configuration')
                    self.connection_attempted = True
                    self.connection_info = self.backend.connect()
                    self.connected = True
                self.record('connect', self.connection_info)
                return dict(owner=self.owner, **self.status())
            # Stop is available to any authenticated operator, including after a lost session.
            if op == 'stop':
                self.stop('operator stop')
                return self.status()
            if not self.owner or not hmac.compare_digest(str(request.get('owner', '')), self.owner):
                raise ValueError('Session does not own this target')
            seq = request.get('sequence')
            if type(seq) is not int or seq <= self.sequence:
                raise ValueError('Stale or duplicate command')
            self.sequence = seq
            if op == 'heartbeat':
                self.heartbeat = time.monotonic()
            elif op == 'dry-run':
                if self.state not in ('prepared','verified','stopped') or not self.candidate: raise ValueError('Prepare a policy first')
                self.dry_run = self.backend.dry_run()
                self.state = 'verified'
                self.record('dry_run', self.dry_run)
            elif op == 'arm':
                if self.state != 'verified' or not self.dry_run: raise ValueError('A successful dry-run is required before arming')
                self.stage, self.command, self.steps = 0, [0.,0.,0.], 0
                self.heartbeat = time.monotonic()
                self.state = 'arming'
                try:
                    self.backend.arm()
                except Exception:
                    self.stop('arming failed')
                    raise
                self.started, self.state = time.monotonic(), 'armed'
                self.heartbeat = self.started
                if self.last_deployed != self.candidate['id']:
                    self.previous = self.last_deployed
                    self.last_deployed = self.candidate['id']
                    self.history.append(dict(id=self.last_deployed, time=time.time(), simulation=self.args.adapter == 'simulator'))
                self.record('arm', self.candidate)
            elif op == 'advance':
                if self.state != 'armed' or time.monotonic()-self.started < 3. or self.steps < 50: raise ValueError('Each stage needs 3 seconds and 50 healthy control steps')
                if self.stage >= len(STAGES)-1: raise ValueError('Already at the final command stage')
                self.stage += 1
                self.started, self.steps = time.monotonic(), 0
                self.record('advance', self.stage)
            elif op == 'command':
                if self.state != 'armed': raise ValueError('Arm the prepared policy before commanding velocity')
                values = request.get('velocity')
                limits = [v*STAGES[self.stage] for v in self.args.command_limits]
                if not isinstance(values,list) or len(values)!=3 or any(type(v) not in (float,int) or not math.isfinite(v) or abs(v)>limit+1e-8 for v,limit in zip(values, limits)):
                    raise ValueError(f'Command exceeds this stage limits: {limits}')
                self.command = values
                self.record('command', values)
            elif op == 'rollback':
                self.stop('rollback')
                if self.state != 'stopped': raise ValueError('Stop is not confirmed; rollback refused')
                if not self.previous:
                    self.backend.rollback()
                    self.candidate, self.dry_run = None, None
                    self.record('rollback', 'target policy restored by adapter')
                else:
                    folder = self.args.state/'bundles'/self.previous
                    verify_bundle(folder)
                    self.backend.prepare(folder)
                    source=json.loads((folder/'provenance.json').read_text())
                    self.candidate=dict(id=self.previous, checkpoint_sha256=source['checkpoint_sha256'],policy_sha256=source['onnx_sha256'],observations=source['observations'],actions=source['actions'])
                    self.dry_run, self.state = None, 'prepared'
                    self.record('rollback', self.candidate)
                # Rollback selects a retained policy and remains disarmed. It never resumes motion.
            else:
                raise ValueError('Unknown agent operation')
            return self.status()

    def loop(self):
        while not self.shutdown:
            t = time.monotonic()
            with self.lock:
                if self.state == 'armed':
                    if t-self.heartbeat > self.args.watchdog:
                        self.stop('heartbeat expired')
                    else:
                        try:
                            self.telemetry = self.backend.step(self.command)
                            self.steps += 1
                            if self.telemetry.get('terminal'): self.stop(self.telemetry.get('terminal_reason','target stopped'))
                            if time.monotonic()-t > .25: raise RuntimeError('Control deadline exceeded 250 ms')
                            with (self.args.state/'telemetry.jsonl').open('a') as f:
                                f.write(json.dumps(dict(time=time.time(), candidate=self.candidate['id'], stage=self.stage, **self.telemetry),allow_nan=False)+'\n')
                        except Exception as error:
                            self.error = str(error)
                            self.stop('control fault: '+str(error))
            time.sleep(max(.001, getattr(self.backend, 'dt', .02)-(time.monotonic()-t)))


class Server(socketserver.ThreadingMixIn, HTTPServer):
    daemon_threads = True


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_): pass
    def do_GET(self): self.handle_request(False)
    def do_POST(self): self.handle_request(True)
    def handle_request(self, post):
        code = 200
        try:
            auth = self.headers.get('Authorization','')
            if not hmac.compare_digest(auth, 'Bearer '+self.server.token):
                self.send_error(401, 'Authentication required')
                return
            if self.path == '/status' and not post:
                with self.server.agent.lock: result=self.server.agent.status()
            elif self.path in ('/command','/bundle') and post:
                length=int(self.headers.get('Content-Length',0))
                limit=LIMIT if self.path=='/bundle' else 16384
                if not 0 < length <= limit: raise ValueError('Invalid request size')
                self.connection.settimeout(15)
                body=self.rfile.read(length)
                if len(body)!=length: raise ValueError('Truncated request')
                result=self.server.agent.upload(body) if self.path=='/bundle' else self.server.agent.request(json.loads(body))
            else: raise ValueError('Unknown endpoint')
        except Exception as error:
            code,result=400,dict(error=str(error))
        encoded=json.dumps(result,allow_nan=False).encode()
        self.send_response(code); self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(encoded)));self.end_headers()
        try:self.wfile.write(encoded)
        except (BrokenPipeError, ConnectionResetError):pass


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--adapter',default='simulator',choices=['simulator','microduck','unitree','open-duck'])
    p.add_argument('--robot',required=True)
    p.add_argument('--state',type=Path,required=True)
    p.add_argument('--token-file',type=Path,required=True)
    p.add_argument('--port',type=int,default=8091)
    p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[2])
    p.add_argument('--hardware-config',type=Path)
    p.add_argument('--watchdog',type=float,default=.8)
    p.add_argument('--command-limits',type=float,nargs=3,default=[.2,.1,.5])
    args=p.parse_args()
    if not .3 <= args.watchdog <= 2 or any(not math.isfinite(v) or not 0 < v <= 2 for v in args.command_limits):p.error('Invalid watchdog or command limits')
    sys.path.insert(0,str(args.root/'tools/training'))
    token=args.token_file.read_text().strip()
    if len(token)<32: p.error('Token must have at least 32 characters')
    agent=Agent(args)
    server=Server(('127.0.0.1',args.port),Handler)
    server.agent,server.token=agent,token
    atomic(args.state/'endpoint.json',dict(url=f'http://127.0.0.1:{server.server_port}',pid=os.getpid(),version=VERSION))
    runner=threading.Thread(target=agent.loop,daemon=True);runner.start()
    def finish(*_):
        with agent.lock:
            if agent.state=='armed':agent.stop('agent exit')
            agent.shutdown=True
        threading.Thread(target=server.shutdown,daemon=True).start()
    signal.signal(signal.SIGTERM,finish);signal.signal(signal.SIGINT,finish)
    try:server.serve_forever(poll_interval=.1)
    finally:
        finish();server.server_close()


if __name__=='__main__':main()
