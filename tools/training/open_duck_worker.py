#!/usr/bin/env python3
"""Run Nexus Open Duck PPO with Apple MPS learning and native CPU MuJoCo."""
import argparse
from collections import deque
from contextlib import redirect_stdout
import copy
import hashlib
import importlib.metadata
import json
from pathlib import Path
import shutil
import statistics
import sys
import time
from types import MethodType

from microduck_motion import atomic_json
import parallel_ppo as parallel
from open_duck_env import COMMIT, DT, LOCKED_JOINTS, OBS_SIZE, ROBOT, TASK, DuckTask, LiveMotion

EVENT_STREAM = sys.stdout


def emit(event, **fields):
    parallel.emit(EVENT_STREAM,event,fields)


def runner_config(seed, interval, task_config=None):
    cfg = dict(seed=seed, num_steps_per_env=24, save_interval=interval, logger="tensorboard",
                obs_groups={"actor": ["state"], "critic": ["state"]},
                actor=dict(class_name="MLPModel", hidden_dims=[256, 256, 128], activation="elu", obs_normalization=True,
                           distribution_cfg=dict(class_name="GaussianDistribution", init_std=0.5, std_type="scalar")),
                critic=dict(class_name="MLPModel", hidden_dims=[256, 256, 128], activation="elu", obs_normalization=True),
                algorithm=dict(class_name="PPO", num_learning_epochs=5, num_mini_batches=4, learning_rate=0.0003,
                               schedule="adaptive", gamma=0.99, lam=0.95, entropy_coef=0.01, desired_kl=0.01,
                               max_grad_norm=1.0, value_loss_coef=1.0, use_clipped_value_loss=True,
                               clip_param=0.2, normalize_advantage_per_mini_batch=False, rnd_cfg=None, symmetry_cfg=None))
    if task_config:
        p=task_config['ppo']
        cfg['num_steps_per_env']=p['steps_per_env'];cfg['save_interval']=p['checkpoint_every']
        cfg['actor']['hidden_dims']=p['hidden'];cfg['critic']['hidden_dims']=p['hidden']
        cfg['algorithm'].update(num_learning_epochs=p['epochs'],num_mini_batches=p['minibatches'],learning_rate=p['learning_rate'],
            gamma=p['gamma'],lam=p['gae_lambda'],entropy_coef=p['entropy'],clip_param=p['clip'])
    return cfg


class DuckVecEnv:
    device = "cpu"
    num_actions = 14
    max_episode_length = 1000

    def __init__(self, task, count, seed, motion):
        import torch
        self.task, self.num_envs, self.motion = task, count, motion
        self.max_episode_length=round(task.config['simulation']['episode_seconds']/task.dt)
        self.episodes = [task.episode(seed + i) for i in range(count)]
        self.episode_length_buf = torch.zeros(count, dtype=torch.long)
        self.cfg = dict(task=TASK, control_dt=task.dt, physics_dt=task.model.opt.timestep, port="Nexus native MuJoCo", commit=COMMIT)
        self.failures = deque(maxlen=100)
        self.lengths = deque(maxlen=100)
        self.rewards = []

    def get_observations(self):
        import numpy as np
        import torch
        from tensordict import TensorDict
        return TensorDict({"state": torch.from_numpy(np.stack([e.obs for e in self.episodes]))}, [self.num_envs])

    def step(self, actions):
        import torch
        result = [e.step(a) for e, a in zip(self.episodes, actions.detach().cpu().numpy())]
        self.rewards.extend(r[1] for r in result)
        dones = [fallen or timeout for _, _, fallen, timeout in result]
        self.motion.step(self.episodes[0], dones[0])
        self.episode_length_buf += 1
        for i, done in enumerate(dones):
            if done:
                self.failures.append(result[i][2])
                self.lengths.append(int(self.episode_length_buf[i]))
                self.episodes[i].reset()
                self.episode_length_buf[i] = 0
        return (self.get_observations(), torch.tensor([r[1] for r in result], dtype=torch.float32),
                torch.tensor(dones, dtype=torch.long),
                {"time_outs": torch.tensor([r[3] for r in result], dtype=torch.float32)})


def export_policy(runner, checkpoint, output):
    import numpy as np
    import onnxruntime as ort
    import torch
    output.mkdir(parents=True, exist_ok=True)
    obs_size=runner.env.task.obs_size
    policy = runner.alg.get_policy().as_jit().cpu().eval()
    onnx_path = output / "policy.onnx"
    torch.onnx.export(policy, torch.zeros(1, obs_size), str(onnx_path), dynamo=False,
                      input_names=["observation"], output_names=["action"], opset_version=18)
    session = ort.InferenceSession(str(onnx_path), providers=["CPUExecutionProvider"])
    if session.get_inputs()[0].shape != [1, obs_size] or session.get_outputs()[0].shape != [1, 14]:
        raise RuntimeError("Exported Open Duck policy shape mismatch")
    maximum_error = 0.0
    for obs in [np.zeros((1, obs_size), np.float32),
                np.random.default_rng(91).normal(0, 0.5, (1, obs_size)).astype(np.float32)]:
        actual = session.run(None, {"observation": obs})[0]
        with torch.inference_mode():
            expected = policy(torch.from_numpy(obs)).numpy()
        if not np.isfinite(actual).all():
            raise RuntimeError("Export returned nonfinite actions")
        maximum_error = max(maximum_error, float(np.max(np.abs(actual - expected))))
    if maximum_error > 1e-5:
        raise RuntimeError(f"ONNX export differs from trained policy: {maximum_error}")
    evidence = dict(robot=ROBOT, commit=COMMIT, checkpoint=str(checkpoint.resolve()),
                    checkpoint_sha256=hashlib.sha256(checkpoint.read_bytes()).hexdigest(),
                    onnx_sha256=hashlib.sha256(onnx_path.read_bytes()).hexdigest(),
                    onnx_shape=f"[1,{obs_size}] -> [1,14]", export_max_error=maximum_error,
                    normalizer="included in exported policy", locked_joints=LOCKED_JOINTS)
    atomic_json(output / "verified.json", evidence)
    return evidence


def train(args):
    import numpy as np
    import torch
    from rsl_rl.runners import OnPolicyRunner
    if importlib.metadata.version("rsl-rl-lib") != "5.0.1":
        raise RuntimeError("Open Duck adapter requires rsl-rl-lib==5.0.1")
    if args.device == "mps" and not torch.backends.mps.is_available():
        raise RuntimeError("Apple MPS is unavailable; no CPU learner fallback was started")
    if args.device == "cuda" and not torch.cuda.is_available():
        raise RuntimeError("CUDA is unavailable; no CPU learner fallback was started")
    torch.set_num_threads(4)
    torch.manual_seed(args.seed)
    np.random.seed(args.seed % (2**32))
    args.run_dir.mkdir(parents=True, exist_ok=True)
    training = args.run_dir / "training"
    training.mkdir(exist_ok=False)
    authored=json.loads(args.task_config.read_text()) if args.task_config else None
    config = runner_config(args.seed, args.evaluate_every, authored)
    atomic_json(args.run_dir / "training-plan.json", dict(robot=ROBOT, task=TASK, commit=COMMIT,
                backend="Nexus native MuJoCo task port / RSL-RL PPO", learner=args.device, physics="cpu",
                num_envs=parallel.total_steps(args.num_envs), num_envs_per_process=args.num_envs, processes=parallel.world(), iterations=args.iterations, env_steps=parallel.total_steps(args.num_envs * args.iterations * config['num_steps_per_env']),
                imitation=True, backlash=False, domain_randomization=False, critic="symmetric actor observation layout",
                locked_joints=LOCKED_JOINTS, task_config=authored, runner=copy.deepcopy(config)))
    emit("status", message="Loading pinned Open Duck model and reference motion")
    snapshot=None
    if authored and args.resume_checkpoint:
        info=torch.load(args.resume_checkpoint,weights_only=True,map_location='cpu').get('infos') or {}
        candidates=[args.resume_checkpoint.parent/'model.mjb',args.resume_checkpoint.parent.parent/'model.mjb']
        snapshot=next((p for p in candidates if p.is_file() and hashlib.sha256(p.read_bytes()).hexdigest()==info.get('model_sha256')),None)
        if snapshot is None: raise ValueError('Continuation needs the matching captured Open Duck model.mjb')
        import mujoco
        if info.get('mujoco_version')!=mujoco.__version__: raise ValueError('Captured Open Duck model requires its original MuJoCo version')
    task = DuckTask(args.repo, authored, snapshot)
    model_hash=None
    if authored:
        import mujoco
        if snapshot: shutil.copyfile(snapshot,args.run_dir/'model.mjb')
        else: mujoco.mj_saveModel(task.model,str(args.run_dir/'model.mjb'),None)
        model_hash=hashlib.sha256((args.run_dir/'model.mjb').read_bytes()).hexdigest()
        atomic_json(args.run_dir/'task.json',authored)
    motion = LiveMotion(args.run_dir, task)
    env = DuckVecEnv(task, args.num_envs, args.seed, motion)
    evaluated = set()
    start_iteration = 0

    class NexusRunner(parallel.RunnerMixin, OnPolicyRunner):
        def save(self, path, infos=None):
            checkpoint = Path(path)
            info=dict(robot=ROBOT, commit=COMMIT, obs_size=task.obs_size, env_steps=parallel.total_steps(env.motion.seq * env.num_envs))
            if authored: info.update(task_config=authored,contract=task.contract,model_sha256=model_hash,mujoco_version=mujoco.__version__)
            super().save(path, info)
            target = args.run_dir / "policy.pt"
            shutil.copyfile(checkpoint, target.with_suffix(".pt.tmp"))
            target.with_suffix(".pt.tmp").replace(target)
            iteration = self.current_learning_iteration + 1
            emit("checkpoint", path=str(target), iteration=iteration)
            if iteration in evaluated:
                return
            evaluated.add(iteration)
            output = args.run_dir / "evaluations" / f"iteration_{iteration:06d}"
            evidence = export_policy(self, checkpoint, output / "training")
            shutil.copyfile(output / "training/policy.onnx", args.run_dir / "policy.onnx")
            emit("status", message=f"Validating Open Duck checkpoint {iteration}")
            from validate_open_duck import validate
            report = validate(task, output / "training/policy.onnx", output, args.run_dir)
            atomic_json(training / "verified.json", dict(**evidence, learner_device=args.device, physics_device="cpu",
                        iterations=iteration-start_iteration, total_iterations=iteration,
                        env_steps=parallel.total_steps(env.motion.seq*env.num_envs),
                        policy_parameter_device=str(next(self.alg.get_policy().parameters()).device),
                        optimizer_states=len(self.alg.optimizer.state),
                        observation_samples=float(self.alg.get_policy().obs_normalizer.count.item()),
                        mps_allocated_bytes=torch.mps.current_allocated_memory() if args.device == "mps" else 0))
            emit("validation", iteration=iteration, survived=report["survived"], total=report["total"],
                 tracking_passed=report["tracking_passed"], result=report["validation_gate"])

    try:
        runner = NexusRunner(env, config, str(training), parallel.device(args.device))
        actual_device = str(next(runner.alg.get_policy().parameters()).device)
        if not actual_device.startswith(args.device):
            raise RuntimeError(f"Policy device {actual_device} differs from requested {args.device}")
        if args.resume_checkpoint:
            saved = torch.load(args.resume_checkpoint, weights_only=False, map_location="cpu")
            info = saved.get("infos") or {}
            if info.get("robot") != ROBOT or info.get("commit") != COMMIT or info.get("obs_size") != task.obs_size:
                raise RuntimeError("Resume checkpoint is not from this Open Duck task contract")
            if bool(authored)!=bool(info.get('task_config')) or (authored and info.get('contract')!=task.contract):
                raise ValueError('Resume checkpoint differs from the captured Open Duck task configuration')
            inputs = args.run_dir / "input"
            inputs.mkdir()
            resume = inputs / args.resume_checkpoint.name
            shutil.copyfile(args.resume_checkpoint, resume)
            runner.load(str(resume), map_location="cpu")
            runner.current_learning_iteration += 1
            start_iteration = runner.current_learning_iteration
            runner.alg.learning_rate = float(runner.alg.optimizer.param_groups[0]["lr"])
            if not runner.alg.optimizer.state:
                raise RuntimeError("PPO optimizer state was not restored")
            emit("resumed", completed_iterations=start_iteration, optimizer_states=len(runner.alg.optimizer.state),
                 observation_samples=float(runner.alg.get_policy().obs_normalizer.count.item()))
        emit("device", learner=actual_device, physics="cpu")
        original_log = runner.logger.log

        def log(_logger, **values):
            original_log(**values)
            emit("sample", step=motion.seq*env.num_envs, iteration=values["it"]-start_iteration+1,
                 total_iteration=values["it"]+1, reward=statistics.mean(env.rewards),
                 reward_metric="mean weighted reward per control step",
                 fall_rate=statistics.mean(env.failures) if env.failures else 0., fall_rate_measured=bool(env.failures),
                 completed_episodes=len(env.failures),
                 episode_len=statistics.mean(env.lengths) if env.lengths else float(env.episode_length_buf.float().mean()),
                 iteration_env_steps=env.num_envs*config['num_steps_per_env'], iteration_seconds=values["collect_time"]+values["learn_time"],
                 steps_per_sec=env.num_envs*config['num_steps_per_env']/(values["collect_time"]+values["learn_time"]),
                 learner_device=actual_device,
                 mps_allocated_bytes=torch.mps.current_allocated_memory() if args.device == "mps" else 0)
            env.rewards.clear()
        runner.logger.log = MethodType(log, runner.logger)
        runner.learn(args.iterations)
        emit("complete", iterations=args.iterations, total_iterations=runner.current_learning_iteration+1)
    finally:
        motion.close()


def main():
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=root / "training-stack/open_duck_playground")
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--num-envs", type=int, default=32)
    parser.add_argument("--iterations", type=int)
    parser.add_argument("--evaluate-every", type=int, default=100)
    parser.add_argument("--seed", type=lambda s: int(s, 16) if s.startswith("0x") else int(s), default=42)
    parser.add_argument("--device", choices=["mps", "cpu", "cuda"], default="mps")
    parser.add_argument("--resume-checkpoint", type=Path)
    parser.add_argument("--task-config", type=Path)
    args = parser.parse_args()
    if args.task_config:
        from duck_task_config import validate
        cfg=json.loads(args.task_config.read_text());validate(cfg)
        if cfg['kind']!='open_duck': raise ValueError('Open Duck worker requires its own task kind')
        args.num_envs=cfg['ppo']['num_envs'];args.device=cfg['ppo']['device'];args.evaluate_every=cfg['ppo']['checkpoint_every']
        args.iterations=args.iterations or cfg['ppo']['iterations']
    else: args.iterations=args.iterations or 1000
    if args.num_envs < 4 or args.iterations < 1 or args.evaluate_every < 1:
        parser.error("Use at least 4 environments and positive iteration/evaluation budgets")
    args.repo, args.run_dir = args.repo.resolve(), args.run_dir.resolve()
    parallel.prepare_args(args)
    try:
        with redirect_stdout(sys.stderr):
            train(args)
    except Exception as error:
        emit("error", message=str(error))
        raise


if __name__ == "__main__":
    main()
