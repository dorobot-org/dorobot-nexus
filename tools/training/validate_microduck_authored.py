"""Evaluate the captured specialized Microduck task with its real BAM dynamics."""
import copy
import hashlib
import json
from pathlib import Path
import numpy as np
import onnxruntime as ort
import torch
from authored_microduck import build_env
from duck_task_config import JOINTS,contract
from microduck_motion import atomic_json,publish_validation


def evaluate(cfg,snapshot,policy,output,run_dir,seconds=10.):
    config=copy.deepcopy(cfg);config['ppo']['device']='cpu'
    cases=[(name,command,seed) for name,command in [('stand',[0.,0.,0.]),('forward_slow',[.1,0.,0.]),('forward',[.2,0.,0.]),
        ('turn_left',[0.,0.,.5]),('turn_right',[0.,0.,-.5])] for seed in [10001,20003,30011]]
    env,_=build_env(config,play=True,num_envs=len(cases),seed=12345,snapshot=snapshot,evaluation=True)
    base=env.unwrapped;robot=base.scene['robot'];ids=[robot.joint_names.index(n) for n in JOINTS]
    directory=Path(output)/'validation';directory.mkdir(parents=True)
    session=ort.InferenceSession(str(policy),providers=['CPUExecutionProvider'])
    dt=base.step_dt;sim=cfg['simulation'];records=[dict(joints=[],base=[],velocities=[],tilts=[],fall=None) for _ in cases]
    commands=torch.tensor([c[1] for c in cases],dtype=torch.float32,device=env.device)
    try:
        root=robot.data.default_root_state.clone();root[:,:3]+=base.scene.env_origins
        root[:,2]=base.scene.env_origins[:,2]+(sim['spawn_height'] or .125)
        joints=robot.data.default_joint_pos.clone()
        for i,(_,_,seed) in enumerate(cases):
            rng=np.random.default_rng(seed);roll,pitch=rng.uniform(-np.radians(2),np.radians(2),2)
            root[i,2]+=float(rng.uniform(-.001,.001));root[i,3:7]=torch.tensor([np.cos(roll/2)*np.cos(pitch/2),np.sin(roll/2)*np.cos(pitch/2),np.cos(roll/2)*np.sin(pitch/2),-np.sin(roll/2)*np.sin(pitch/2)],dtype=torch.float32)
            joints[i,ids]+=torch.tensor(rng.uniform(-.01,.01,14),dtype=torch.float32)
        robot.write_root_state_to_sim(root);robot.write_joint_state_to_sim(joints,torch.zeros_like(joints))
        for actuator in robot.actuators:actuator.reset()
        base.sim.forward();base.sim.sense()
        def set_commands():
            base.command_manager.get_command('twist')[:]=commands
            for name in ('head_pose','body_pose'):base.command_manager.get_command(name)[:]=0.
            # Commands/root state were changed after mjlab cached its previous
            # observation. Recompute without advancing observation history.
            base.observation_manager._obs_buffer=None
        for frame in range(round(seconds/dt)+1):
            set_commands()
            pos=(robot.data.root_link_pos_w-base.scene.env_origins).detach().cpu().numpy()
            quat=robot.data.root_link_quat_w.detach().cpu().numpy();angles=robot.data.joint_pos[:,ids].detach().cpu().numpy()
            linear=robot.data.root_link_lin_vel_b.detach().cpu().numpy();angular=robot.data.root_link_ang_vel_b.detach().cpu().numpy()
            if not all(np.isfinite(a).all() for a in (pos,quat,angles,linear,angular)):raise RuntimeError('Nonfinite Microduck validation physics')
            for i,record in enumerate(records):
                if record['fall'] is not None:continue
                record['joints'].append(angles[i].tolist());record['base'].append([*pos[i].tolist(),*quat[i,1:].tolist(),float(quat[i,0])])
                tilt=float(np.degrees(np.arccos(np.clip(1-2*(quat[i,1]**2+quat[i,2]**2),-1,1))))
                record['tilts'].append(tilt);record['velocities'].append([*linear[i,:2].tolist(),float(angular[i,2])])
                if tilt>sim['max_tilt_degrees'] or pos[i,2]<sim['min_height']:record['fall']=frame*dt
            if frame==round(seconds/dt) or all(r['fall'] is not None for r in records):break
            obs=env.get_observations()['actor'].detach().cpu().numpy()
            # Official exports have a fixed batch dimension of one.
            actions=np.stack([session.run(None,{session.get_inputs()[0].name:o[None]})[0][0] for o in obs])
            if not np.isfinite(actions).all():raise RuntimeError('Nonfinite Microduck policy action')
            env.step(torch.from_numpy(actions))
        results=[]
        for (name,command,seed),record in zip(cases,records):
            velocity=np.asarray(record['velocities']);rmse=np.sqrt(np.mean((velocity-command)**2,axis=0));survived=record['fall'] is None
            rollout=directory/f'{name}_seed{seed}.json'
            atomic_json(rollout,dict(name=f'Microduck {name} seed {seed}',dt=dt,joint_names=JOINTS,joints=record['joints'],base=record['base']))
            results.append(dict(case=name,seed=seed,command=command,survived=survived,tracking_pass=survived and bool(np.all(rmse<=[.05,.05,.2])),
                max_tilt_deg=max(record['tilts']),first_fall_seconds=record['fall'],recorded_seconds=(len(record['joints'])-1)*dt,
                upright_mean_velocity=velocity.mean(axis=0).tolist(),velocity_rmse=rmse.tolist(),rollout=str(rollout.resolve())))
        report=dict(status='completed',robot='Microduck',contract=contract(cfg),controller='Official mjlab task and BAM motor model with captured edits',
            policy_sha256=hashlib.sha256(Path(policy).read_bytes()).hexdigest(),cases=results,total=len(results),survived=sum(c['survived'] for c in results),
            tracking_passed=sum(c['tracking_pass'] for c in results),duration_seconds=seconds,thresholds=dict(tilt_degrees=sim['max_tilt_degrees'],height=sim['min_height'],velocity_rmse=[.05,.05,.2]))
        report['validation_gate']='PASS' if report['tracking_passed']==len(results) else 'FAIL'
        atomic_json(directory/'report.json',report)
        lines=['# Captured Microduck task validation','',f"Result: **{report['validation_gate']}**. Survival {report['survived']}/{len(results)}; tracking {report['tracking_passed']}/{len(results)}.",'',
            'Exported policy in the original mjlab/BAM task with captured edits. Three held-out pose seeds per command, zero head/body commands, no scheduled pushes; each trial ends at its first fall. This is simulator acceptance, not physical hardware validation.','',
            '| Command | Seed | Survived | Tracking | Velocity RMSE x / y / yaw |','|---|---:|---|---|---|']
        lines += [f"| {c['case']} | {c['seed']} | {c['survived']} | {c['tracking_pass']} | "+' / '.join(f'{v:.3f}' for v in c['velocity_rmse'])+' |' for c in results]
        (directory/'report.md').write_text('\n'.join(lines)+'\n');publish_validation(run_dir,output,robot='Microduck');return report
    finally:env.close()
