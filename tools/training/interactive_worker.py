#!/usr/bin/env python3
"""Live policy physics controlled by acknowledged JSON lines on stdin.

The GUI owns the pipe. EOF stops simulation and closes its measured JSONL log.
Every control step is recorded; paused wall time never advances physics time.
"""
import argparse
import json
import math
from pathlib import Path
import queue
import signal
import sys
import threading
import time
from types import SimpleNamespace

import numpy as np
from microduck_motion import atomic_json

MAX_FRAMES = 100_000
MAX_BYTES = 256 * 1024 * 1024


def vector(value, limit, name):
    if not isinstance(value, list) or len(value) != 3:
        raise ValueError(f'{name} requires three numbers')
    result = np.array(value, dtype=float)
    if not np.isfinite(result).all() or np.max(np.abs(result)) > limit:
        raise ValueError(f'{name} must be finite and within ±{limit}')
    return result


class Session:
    def __init__(self, adapter, policy, output, identity):
        self.adapter, self.policy, self.output, self.identity = adapter, policy, Path(output), identity
        self.command = np.zeros(3)
        self.force = np.zeros(3)
        self.force_steps = self.sequence = self.resets = self.ack = self.bytes = 0
        self.state, self.error, self.terminal = 'paused', '', False
        self.log = (self.output / 'simulation-motion.jsonl').open('x', buffering=1)
        self.packet = self.pose_packet()
        self.publish()

    def pose_packet(self):
        return dict(version=1, source='simulation', **self.identity, sequence=self.sequence,
                    reset_count=self.resets, sim_time=self.sequence*self.adapter.dt,
                    dt=self.adapter.dt, env_index=0, joint_names=self.adapter.names,
                    command=self.command.tolist(), wall_time=time.time(), **self.adapter.pose())

    def publish(self):
        atomic_json(self.output / 'status.json', dict(version=1, **self.identity, state=self.state,
                    sequence=self.sequence, reset_count=self.resets, ack=self.ack, error=self.error,
                    terminal=self.terminal, command=self.command.tolist(),
                    force_world=self.force.tolist(), force_steps=self.force_steps, pose=self.packet))

    def control(self, request):
        try:
            if not isinstance(request, dict) or not isinstance(request.get('id'), int) or request['id'] <= self.ack:
                raise ValueError('Control id must be a new increasing integer')
            self.ack = request['id']
            op = request.get('op')
            if op == 'command':
                self.command = vector(request.get('value'), 10., 'Body velocity command (m/s, m/s, rad/s)')
            elif op == 'push':
                force = vector(request.get('force'), 1000., 'World force in newtons')
                duration = float(request.get('seconds', .1))
                if not math.isfinite(duration) or not 0 < duration <= 5.:
                    raise ValueError('Push duration must be in (0, 5] seconds')
                self.force = force
                self.force_steps = math.ceil(duration/self.adapter.dt)
            elif op == 'pause':
                self.state = 'paused'
            elif op in ('resume', 'step'):
                if self.terminal:
                    raise ValueError('Episode ended; restart before advancing physics')
                self.state = 'running' if op == 'resume' else 'paused'
                if op == 'step':
                    self.step()
            elif op == 'restart':
                self.adapter.reset(self.command)
                self.force[:] = 0.
                self.force_steps = 0
                self.resets += 1
                self.state, self.terminal = 'paused', False
                self.packet = self.pose_packet()
            elif op == 'stop':
                self.state = 'stopped'
            else:
                raise ValueError(f'Unknown simulation command: {op}')
            self.error = ''
        except (ValueError, TypeError, OverflowError) as e:
            self.error = str(e)
        self.publish()

    def step(self):
        if self.sequence >= MAX_FRAMES or self.bytes >= MAX_BYTES:
            self.state, self.error = 'stopped', 'Recording limit reached; start a new session'
            self.publish()
            return
        observation = self.adapter.observe(self.command)
        action = np.asarray(self.policy(observation), dtype=np.float32)
        if not np.isfinite(observation).all() or not np.isfinite(action).all():
            raise ValueError('Policy returned nonfinite observations or actions')
        force = self.force.copy() if self.force_steps else np.zeros(3)
        physical = self.adapter.step(action, force)
        self.sequence += 1
        packet = dict(**self.pose_packet(), **{k: v for k, v in physical.items() if k not in ('joints', 'base')},
                      observation=observation.tolist(), observation_names=self.adapter.obs_names,
                      action=action.tolist(), actuator_joints=self.adapter.actuator_names,
                      force_world=force.tolist(), control_ack=self.ack,
                      sample_timing='observation before policy action; pose/target/torque after control step; torque from last physics substep; world force held throughout control step; no automatic reset')
        line = json.dumps(packet, allow_nan=False) + '\n'
        if self.bytes + len(line.encode()) > MAX_BYTES:
            self.sequence -= 1
            self.state, self.error = 'stopped', 'Recording byte limit reached; final unrecorded step is not published'
            self.publish()
            return
        self.log.write(line)
        self.bytes += len(line.encode())
        self.packet = packet
        self.terminal = physical['terminal']
        if self.terminal:
            self.state = 'paused'
        self.force_steps = max(0, self.force_steps-1)
        if not self.force_steps:
            self.force[:] = 0.

    def close(self):
        self.state = 'stopped' if self.state != 'failed' else self.state
        self.log.close()
        self.publish()
        self.adapter.close()


def run(args):
    import torch
    import onnxruntime as ort
    from export_checkpoint import export, sha
    from interactive_env import load
    torch.set_num_threads(2)
    args.output.mkdir(parents=True, exist_ok=False)
    identity = dict(session_id=args.output.name, run_id=args.run_id, robot=args.robot,
                    robot_id=args.robot_id, checkpoint=str(args.checkpoint.resolve()),
                    checkpoint_sha256=sha(args.checkpoint), seed=args.seed)
    atomic_json(args.output / 'status.json', dict(version=1, **identity, state='loading', sequence=0, ack=0))
    inbox = queue.Queue(maxsize=1024)
    def read():
        try:
            for line in sys.stdin:
                if len(line) > 16384:
                    raise ValueError('Simulation control exceeds 16 KiB')
                try:
                    inbox.put(json.loads(line), timeout=1.)
                except json.JSONDecodeError:
                    inbox.put({'op': 'invalid JSON'})
        finally:
            inbox.put(None)
    threading.Thread(target=read, daemon=True).start()
    def stopped(*_):
        try:
            inbox.put_nowait(None)
        except queue.Full:
            pass
    signal.signal(signal.SIGTERM, stopped)
    signal.signal(signal.SIGINT, stopped)
    session = None
    try:
        bundle = args.output / 'policy'
        export(SimpleNamespace(checkpoint=args.checkpoint, output=bundle, robot=args.robot,
                               studio_root=args.studio_root, recipe_file=args.recipe_file))
        provenance = json.loads((bundle / 'provenance.json').read_text())
        if provenance['checkpoint_sha256'] != identity['checkpoint_sha256']:
            raise ValueError('Selected checkpoint changed while simulation was starting')
        identity['policy_sha256'] = provenance['onnx_sha256']
        adapter = load(bundle, args.studio_root, args.robot, args.seed)
        options = ort.SessionOptions()
        options.intra_op_num_threads = 1
        network = ort.InferenceSession(str(bundle / 'policy.onnx'), options, providers=['CPUExecutionProvider'])
        def policy(observation):
            return network.run(None, {network.get_inputs()[0].name: observation[None].astype(np.float32)})[0][0]
        # Test dimensions and inference before reporting the session ready.
        action = policy(adapter.observe(np.zeros(3)))
        if action.shape != (provenance['actions'],) or not np.isfinite(action).all():
            raise ValueError('Policy dimensions disagree with the captured task')
        session = Session(adapter, policy, args.output, identity)
        next_step = time.monotonic()
        while session.state not in ('stopped', 'failed'):
            timeout = max(0., min(.1, next_step-time.monotonic())) if session.state == 'running' else .1
            try:
                request = inbox.get(timeout=timeout)
                if request is None:
                    break
                session.control(request)
                # No accumulated wall-clock steps on resume or after slow physics.
                next_step = time.monotonic()
            except queue.Empty:
                if session.state == 'running':
                    began = time.monotonic()
                    session.step()
                    session.publish()
                    next_step = max(began+adapter.dt, time.monotonic())
    except BaseException as e:
        if session:
            session.state, session.error = 'failed', str(e)
        else:
            atomic_json(args.output / 'status.json', dict(version=1, **identity, state='failed', error=str(e), sequence=0, ack=0))
        raise
    finally:
        if session:
            session.close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--checkpoint', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--run-id', required=True)
    parser.add_argument('--robot', required=True)
    parser.add_argument('--robot-id', required=True)
    parser.add_argument('--recipe-file', type=Path)
    parser.add_argument('--seed', type=int, default=42)
    parser.add_argument('--studio-root', type=Path, default=Path(__file__).resolve().parents[2])
    run(parser.parse_args())
