#!/usr/bin/env python3
"""Held-out robot policy evaluation and paired checkpoint comparison."""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import sys
from types import SimpleNamespace

import numpy as np
from microduck_motion import atomic_json


def policy(bundle):
    import onnxruntime as ort
    options = ort.SessionOptions()
    options.intra_op_num_threads = 1
    net = ort.InferenceSession(str(bundle/'policy.onnx'), options, providers=['CPUExecutionProvider'])
    return lambda obs: net.run(None, {net.get_inputs()[0].name: obs[None].astype(np.float32)})[0][0]


def physical_contract(adapter):
    """Compare semantic model arrays, not zip timestamps or local asset paths."""
    model = (adapter.model if hasattr(adapter, 'model') else adapter.task.model if hasattr(adapter, 'task') else adapter.base.sim.mj_model)
    digest = hashlib.sha256()
    for key in ('body_mass', 'body_inertia', 'body_pos', 'body_quat', 'jnt_type', 'jnt_pos', 'jnt_axis', 'jnt_range',
                'geom_type', 'geom_size', 'geom_pos', 'geom_quat', 'geom_friction', 'geom_contype', 'geom_conaffinity',
                'dof_damping', 'dof_armature', 'dof_frictionloss', 'actuator_gainprm', 'actuator_biasprm',
                'mesh_vert', 'mesh_face'):
        digest.update(key.encode())
        digest.update(np.ascontiguousarray(getattr(model, key)).tobytes())
    cfg = adapter.cfg or {}
    # Actuator calibration affects dynamics outside MuJoCo too (Microduck BAM).
    config = {k: cfg.get(k) for k in ('kind', 'simulation', 'actuators')}
    config.update(dt=adapter.dt, joints=adapter.names)
    digest.update(json.dumps(config, sort_keys=True).encode())
    return digest.hexdigest()


def summarize(results):
    n = len(results)
    survival = sum(r['survived'] for r in results)
    p, z = survival/n, 1.959963984540054
    center = (p+z*z/(2*n))/(1+z*z/n)
    half = z*np.sqrt(p*(1-p)/n+z*z/(4*n*n))/(1+z*z/n)
    return dict(total=n, survived=survival, tracking_passed=sum(r['tracking_passed'] for r in results),
                survival_ci95=[float(center-half), float(center+half)],
                mean_velocity_rmse=np.mean([r['velocity_rmse'] for r in results], axis=0).tolist())


def paired(a, b):
    delta = np.array([r['velocity_rmse'] for r in b])-np.array([r['velocity_rmse'] for r in a])
    rng = np.random.default_rng(823)
    means = delta[rng.integers(0, len(a), (2000, len(a)))].mean(axis=1)
    return dict(convention='B minus A; negative tracking error delta favors B', pairs=len(a),
                mean_rmse_delta=delta.mean(axis=0).tolist(),
                paired_bootstrap_ci95=np.quantile(means, [.025, .975], axis=0).T.tolist(),
                survival_delta=sum(r['survived'] for r in b)/len(b)-sum(r['survived'] for r in a)/len(a),
                method='2000 deterministic paired bootstrap resamples over matched command/seed trials; Wilson interval for survival')


def evaluate(args):
    import torch
    from export_checkpoint import export, sha
    from interactive_env import load
    torch.set_num_threads(2)
    seeds = [int(s.strip()) for s in args.seeds.split(',')]
    if not 1 <= len(seeds) <= 8 or len(set(seeds)) != len(seeds) or any(s < 0 or s > 2**32-1 for s in seeds):
        raise ValueError('Use 1–8 distinct seeds between 0 and 4294967295')
    if not np.isfinite(args.seconds) or not .2 <= args.seconds <= 30.:
        raise ValueError('Evaluation duration must be in [0.2, 30] seconds')
    args.output.mkdir(parents=True, exist_ok=False)
    status = dict(version=1, run_id=args.run_id, robot=args.robot, robot_id=args.robot_id, state='exporting', completed=0)
    publish = lambda: atomic_json(args.output/'status.json', status)
    publish()
    checkpoints = [args.checkpoint] + ([args.compare] if args.compare else [])
    cases = [(name, command, seed) for name, command in [('stand', [0.,0.,0.]), ('forward_slow', [.1,0.,0.]),
             ('forward', [.2,0.,0.]), ('turn_left', [0.,0.,.5]), ('turn_right', [0.,0.,-.5])] for seed in seeds]
    status['total'] = len(cases)*len(checkpoints)
    threshold = [.1, .1, .3] if args.robot == 'Unitree G1-29dof' else [.05, .05, .2]
    models, provenance, contracts = [], [], []
    try:
        for i, checkpoint in enumerate(checkpoints):
            if (args.output/'cancel').exists() or args.output.with_suffix('.cancel').exists(): raise InterruptedError('Evaluation cancelled')
            bundle = args.output/f'policy-{i}'
            export(SimpleNamespace(checkpoint=checkpoint, output=bundle, robot=args.robot, studio_root=args.studio_root, recipe_file=None))
            source = json.loads((bundle/'provenance.json').read_text())
            env = load(bundle, args.studio_root, args.robot, seeds[0])
            models.append(env)
            contracts.append(physical_contract(env))
            provenance.append(dict(checkpoint=str(checkpoint.resolve()), checkpoint_sha256=source['checkpoint_sha256'],
                                   policy_sha256=source['onnx_sha256'], bundle=bundle.name))
        if len(set(contracts)) != 1:
            raise ValueError('Checkpoints have different captured physics/actuator contracts; paired comparison requires matching conditions')
        policies = [policy(args.output/p['bundle']) for p in provenance]
        all_results = [[] for _ in models]
        for case_index, (name, command, seed) in enumerate(cases):
            for i, (env, actor) in enumerate(zip(models, policies)):
                status.update(state='evaluating', policy=i, case=name, seed=seed)
                publish()
                env.seed = seed
                env.reset(command)
                replay = dict(name=f'{args.robot} · {name} · seed {seed} · policy {i}', dt=env.dt,
                              joint_names=env.names, joints=[], base=[])
                velocity, tilt = [], []
                fell, fall_time = False, None
                source = args.output/f'case-{case_index}-policy-{i}.jsonl'
                with source.open('x') as out:
                    for frame in range(int(np.ceil(args.seconds/env.dt))):
                        if (args.output/'cancel').exists() or args.output.with_suffix('.cancel').exists(): raise InterruptedError('Evaluation cancelled')
                        observation = env.observe(command)
                        action = np.asarray(actor(observation), dtype=np.float32)
                        if not np.isfinite(action).all(): raise ValueError('Nonfinite policy action')
                        state = env.step(action, [0.,0.,0.])
                        replay['joints'].append(state['joints'][0])
                        replay['base'].append(state['base'][0])
                        v = env.velocity()
                        velocity.append([float(v[3]), float(v[4]), float(v[2])])
                        x, y = state['base'][0][3:5]
                        tilt.append(float(np.degrees(np.arccos(np.clip(1-2*(x*x+y*y), -1, 1)))))
                        packet = dict(version=1, source='validation', robot=args.robot, robot_id=args.robot_id, run_id=args.run_id,
                                      checkpoint_sha256=provenance[i]['checkpoint_sha256'], seed=seed, command=command,
                                      sequence=frame+1, sim_time=(frame+1)*env.dt, dt=env.dt, joint_names=env.names,
                                      observation=observation.tolist(), action=action.tolist(), velocity=velocity[-1], **state)
                        out.write(json.dumps(packet, allow_nan=False)+'\n')
                        if state['terminal']:
                            fell = state['terminal_reason'] == 'fall'
                            if fell: fall_time = (frame+1)*env.dt
                            break
                survived = not fell and len(velocity)*env.dt >= args.seconds-1e-8
                rmse = np.sqrt(np.mean((np.asarray(velocity)-command)**2, axis=0)).tolist()
                path = args.output/f'case-{case_index}-policy-{i}.json'
                atomic_json(path, replay)
                result = dict(case=name, seed=seed, command=command, survived=survived,
                              tracking_passed=survived and bool(np.all(np.asarray(rmse) <= threshold)),
                              velocity_rmse=rmse, max_tilt_degrees=max(tilt), first_fall_seconds=fall_time,
                              recorded_seconds=len(velocity)*env.dt, frames=len(velocity), rollout=path.name,
                              telemetry=source.name, telemetry_sha256=sha(source), rollout_sha256=sha(path))
                all_results[i].append(result)
                status['completed'] += 1
                publish()
        report = dict(version=1, robot=args.robot, robot_id=args.robot_id, run_id=args.run_id, status='completed',
                      seconds=args.seconds, seeds=seeds, thresholds=dict(velocity_rmse=threshold),
                      physical_contract=contracts[0], checkpoints=provenance, results=all_results,
                      summaries=[summarize(r) for r in all_results], comparison=paired(*all_results) if len(all_results)==2 else None,
                      inference='normalized ONNX on CPU; real captured robot physics', hardware_validated=False)
        atomic_json(args.output/'report.json', report)
        with (args.output/'cases.csv').open('w') as file:
            writer = csv.writer(file)
            writer.writerow(['policy', 'checkpoint_sha256', 'case', 'seed', 'survived', 'tracking_passed', 'rmse_x', 'rmse_y', 'rmse_yaw', 'first_fall_seconds'])
            for i, results in enumerate(all_results):
                for r in results: writer.writerow([i, provenance[i]['checkpoint_sha256'], r['case'], r['seed'], r['survived'], r['tracking_passed'], *r['velocity_rmse'], r['first_fall_seconds']])
        lines = ['# Policy evaluation', '', f'Robot: {args.robot}. Each trial requests {args.seconds:g}s. Seeds: {seeds}.', '',
                 'Metrics use measured control transitions and end at the first fall or task limit. Survival requires the full requested duration. Tracking requires survival and every axis within its RMSE threshold. No hardware acceptance is implied.', '',
                 '| Policy | Survival | Tracking | Mean RMSE x / y / yaw |', '|---|---:|---:|---|']
        for i, summary in enumerate(report['summaries']):
            lines.append(f"| {i}: {provenance[i]['checkpoint_sha256']} | {summary['survived']}/{summary['total']} | {summary['tracking_passed']}/{summary['total']} | {summary['mean_velocity_rmse']} |")
        if report['comparison']: lines += ['', json.dumps(report['comparison'], indent=2)]
        (args.output/'report.md').write_text('\n'.join(lines)+'\n')
        status.update(state='completed', report='report.json')
        publish()
    except BaseException as error:
        status.update(state='cancelled' if isinstance(error, InterruptedError) else 'failed', error=str(error))
        publish()
        raise
    finally:
        for model in models: model.close()


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--checkpoint', type=Path, required=True)
    p.add_argument('--compare', type=Path)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--robot', required=True)
    p.add_argument('--robot-id', required=True)
    p.add_argument('--run-id', required=True)
    p.add_argument('--seconds', type=float, default=10.)
    p.add_argument('--seeds', default='10001,20003,30011')
    p.add_argument('--studio-root', type=Path, default=Path(__file__).resolve().parents[2])
    evaluate(p.parse_args())
