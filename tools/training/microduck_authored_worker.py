#!/usr/bin/env python3
"""Train a captured editable Microduck recipe using its official mjlab/BAM task."""
import argparse
from collections import deque
from contextlib import redirect_stdout
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import shutil
import statistics
import sys
from types import MethodType

from microduck_worker import COMMIT,TASK,check_source
from microduck_motion import atomic_json,TrainingMotion
from duck_task_config import validate,contract,obs_size
import parallel_ppo as parallel

EVENT_STREAM=sys.stdout
def emit(event,**values):parallel.emit(EVENT_STREAM,event,values)
def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def train(args):
    import mujoco
    import torch
    import numpy as np
    from authored_microduck import build_env,capture_source
    from mjlab.rl import MjlabOnPolicyRunner
    from export_checkpoint import export_duck,verify
    from validate_microduck_authored import evaluate
    cfg=json.loads(args.task_config.read_text());validate(cfg)
    if cfg['kind']!='microduck':raise ValueError('Microduck worker requires its own task kind')
    check_source(args.repo)
    p=cfg['ppo'];device=p['device'];budget=p['iterations'] if args.iterations is None else args.iterations
    if not 1<=budget<=1000000:raise ValueError('Iteration budget must be positive')
    if (device=='mps' and not torch.backends.mps.is_available()) or (device=='cuda' and not torch.cuda.is_available()):
        raise RuntimeError(f'Requested {device} learner is unavailable; no fallback was started')
    torch.set_num_threads(4);torch.manual_seed(args.seed);np.random.seed(args.seed%2**32)
    args.run_dir.mkdir(parents=True,exist_ok=True)
    training=args.run_dir/'training';training.mkdir(exist_ok=False)
    snapshot=args.run_dir/'model-source.mjz';resume=None
    if args.resume_checkpoint:
        info=torch.load(args.resume_checkpoint,map_location='cpu',weights_only=True).get('infos') or {}
        if info.get('contract')!=contract(cfg) or info.get('commit')!=COMMIT:
            raise ValueError('Resume checkpoint differs from the captured Microduck task contract')
        candidates=[args.resume_checkpoint.parent/'model-source.mjz',args.resume_checkpoint.parent.parent/'model-source.mjz']
        source=next((v for v in candidates if v.is_file() and sha(v)==info.get('source_model_sha256')),None)
        if source is None:raise ValueError('Microduck continuation requires its matching captured model-source.mjz')
        shutil.copyfile(source,snapshot)
        inputs=args.run_dir/'input';inputs.mkdir();resume=inputs/args.resume_checkpoint.name;shutil.copyfile(args.resume_checkpoint,resume)
    else:capture_source(snapshot)
    atomic_json(args.run_dir/'task.json',cfg)
    env,agent=build_env(cfg,seed=args.seed,snapshot=snapshot)
    agent.seed=args.seed;agent.max_iterations=budget
    mujoco.mj_saveModel(env.unwrapped.sim.mj_model,str(args.run_dir/'model.mjb'),None)
    model_hash=sha(args.run_dir/'model.mjb');source_hash=sha(snapshot)
    actual_obs=env.get_observations()['actor'].shape[-1]
    if actual_obs!=obs_size(cfg) or env.num_actions!=14:raise ValueError('Microduck simulator policy dimensions disagree with the captured configuration')
    atomic_json(args.run_dir/'training-plan.json',dict(robot='Microduck',task=TASK,commit=COMMIT,task_config=cfg,
        iterations=budget,num_envs=parallel.total_steps(p['num_envs']),num_envs_per_process=p['num_envs'],processes=parallel.world(),env_steps=parallel.total_steps(budget*p['num_envs']*p['steps_per_env']),learner=device,
        physics=str(env.device),runner=asdict(agent),source_model_sha256=source_hash,model_sha256=model_hash))
    motion=TrainingMotion(env,args.run_dir,'training')
    motion.attach(env)
    start_iteration=0;evaluated=set();failures=deque(maxlen=100)

    class Runner(parallel.RunnerMixin, MjlabOnPolicyRunner):
        def save(self,path,infos=None):
            super().save(path,dict(robot='Microduck',commit=COMMIT,task_config=cfg,contract=contract(cfg),obs_size=actual_obs,
                model_sha256=model_hash,source_model_sha256=source_hash,mujoco_version=mujoco.__version__))
            checkpoint=Path(path);target=args.run_dir/'policy.pt'
            shutil.copyfile(checkpoint,target.with_suffix('.pt.tmp'));target.with_suffix('.pt.tmp').replace(target)
            iteration=self.current_learning_iteration+1;emit('checkpoint',path=str(target),iteration=iteration)
            if iteration in evaluated:return
            evaluated.add(iteration)
            output=args.run_dir/'evaluations'/f'iteration_{iteration:06d}';out=output/'training';out.mkdir(parents=True)
            emit('status',message=f'Exporting captured Microduck checkpoint {iteration} through the official exporter')
            with parallel.single_process_export():
                model,n_obs,n_act,_,_=export_duck('Microduck',checkpoint,out/'policy.onnx',args.repo.parent.parent)
            maximum=verify(model,out/'policy.onnx',n_obs,n_act)
            evidence=dict(robot='Microduck',checkpoint=str(checkpoint.resolve()),checkpoint_sha256=sha(checkpoint),onnx_sha256=sha(out/'policy.onnx'),
                export_max_error=maximum,contract=contract(cfg),source_model_sha256=source_hash,model_sha256=model_hash,
                learner_device=device,physics_device=str(env.device),iterations=iteration-start_iteration,total_iterations=iteration,
                env_steps=parallel.total_steps(motion.seq*p['num_envs']),policy_parameter_device=str(next(self.alg.get_policy().parameters()).device),
                optimizer_states=len(self.alg.optimizer.state),observation_samples=float(self.alg.get_policy().obs_normalizer.count.item()))
            atomic_json(out/'verified.json',evidence);shutil.copyfile(out/'policy.onnx',args.run_dir/'policy.onnx')
            emit('status',message=f'Validating captured Microduck checkpoint {iteration}')
            report=evaluate(cfg,snapshot,out/'policy.onnx',output,args.run_dir)
            atomic_json(training/'verified.json',evidence)
            emit('validation',iteration=iteration,survived=report['survived'],total=report['total'],tracking_passed=report['tracking_passed'],result=report['validation_gate'])

    try:
        runner=Runner(env,asdict(agent),str(training),parallel.device(device))
        actual=str(next(runner.alg.get_policy().parameters()).device)
        if not actual.startswith(device):raise RuntimeError('Requested Microduck policy device was not allocated')
        if resume:
            runner.load(str(resume),map_location='cpu');runner.current_learning_iteration+=1;start_iteration=runner.current_learning_iteration
            runner.alg.learning_rate=float(runner.alg.optimizer.param_groups[0]['lr'])
            if not runner.alg.optimizer.state:raise RuntimeError('Microduck optimizer state was not restored')
            emit('resumed',completed_iterations=start_iteration,optimizer_states=len(runner.alg.optimizer.state),
                observation_samples=float(runner.alg.get_policy().obs_normalizer.count.item()),curriculum_step=env.unwrapped.common_step_counter)
        emit('device',learner=actual,physics=str(env.device))
        old_step,old_log=runner.logger.process_env_step,runner.logger.log
        step_rewards=[];episode_lengths=deque(maxlen=100);counts=torch.zeros(env.num_envs,device=env.device)
        def step(logger,rewards,dones,extras,intrinsic_rewards=None):
            counts.add_(1);finished=dones>0;episode_lengths.extend(counts[finished.to(counts.device)].cpu().tolist());counts[finished.to(counts.device)]=0
            step_rewards.append(float(rewards.mean()))
            finished=dones>0;failures.extend((~extras['time_outs'].to(finished.device).bool())[finished].cpu().tolist())
            old_step(rewards,dones,extras,intrinsic_rewards)
        def log(logger,**values):
            old_log(**values)
            emit('sample',step=motion.seq*p['num_envs'],iteration=values['it']-start_iteration+1,total_iteration=values['it']+1,
                reward=statistics.mean(step_rewards),reward_metric='mean weighted reward per control step',
                fall_rate=statistics.mean(failures) if failures else 0.,fall_rate_measured=bool(failures),
                completed_episodes=len(failures),
                episode_len=statistics.mean(episode_lengths) if episode_lengths else float(counts.mean()),
                iteration_env_steps=p['num_envs']*p['steps_per_env'],iteration_seconds=values['collect_time']+values['learn_time'],
                steps_per_sec=p['num_envs']*p['steps_per_env']/(values['collect_time']+values['learn_time']),learner_device=actual)
            step_rewards.clear()
        runner.logger.process_env_step=MethodType(step,runner.logger);runner.logger.log=MethodType(log,runner.logger)
        runner.learn(budget,init_at_random_ep_len=True)
        emit('complete',iterations=budget,total_iterations=runner.current_learning_iteration+1)
    finally:motion.close();env.close()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--task-config',type=Path,required=True);parser.add_argument('--run-dir',type=Path,required=True)
    parser.add_argument('--repo',type=Path,default=Path(__file__).resolve().parents[2]/'training-stack/microduck_rl')
    parser.add_argument('--iterations',type=int);parser.add_argument('--seed',type=lambda s:int(s,0),default=42)
    parser.add_argument('--resume-checkpoint',type=Path)
    args=parser.parse_args();args.run_dir=args.run_dir.resolve();args.repo=args.repo.resolve()
    parallel.prepare_args(args)
    try:
        with redirect_stdout(sys.stderr):train(args)
    except Exception as e:emit('error',message=str(e));raise


if __name__=='__main__':main()
