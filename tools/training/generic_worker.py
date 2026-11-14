#!/usr/bin/env python3
"""Train a captured Nexus URDF task with CPU MuJoCo and an explicit PPO device."""
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

import numpy as np
from generic_env import Task, validate_config
from microduck_motion import atomic_json
import parallel_ppo as parallel

EVENT_STREAM = sys.stdout


def emit(event, **values):
    parallel.emit(EVENT_STREAM,event,values)


def sha(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def runner_config(cfg, seed):
    p = cfg["ppo"]
    return dict(seed=seed, num_steps_per_env=p["steps_per_env"], save_interval=p["checkpoint_every"], logger="tensorboard",
        obs_groups={"actor": ["state"], "critic": ["state"]},
        actor=dict(class_name="MLPModel", hidden_dims=p["hidden"], activation="elu", obs_normalization=True,
                   distribution_cfg=dict(class_name="GaussianDistribution", init_std=.5, std_type="scalar")),
        critic=dict(class_name="MLPModel", hidden_dims=p["hidden"], activation="elu", obs_normalization=True),
        algorithm=dict(class_name="PPO", num_learning_epochs=p["epochs"], num_mini_batches=p["minibatches"], learning_rate=p["learning_rate"],
            schedule="adaptive", gamma=p["gamma"], lam=p["gae_lambda"], entropy_coef=p["entropy"], desired_kl=.01,
            max_grad_norm=1., value_loss_coef=1., use_clipped_value_loss=True, clip_param=p["clip"],
            normalize_advantage_per_mini_batch=False, rnd_cfg=None, symmetry_cfg=None))


class Motion:
    def __init__(self, root, task):
        self.root, self.task = root, task
        self.seq = self.resets = 0
        self.last = 0.
        self.packet = None
        self.log = (root/"training-motion.jsonl").open("x", buffering=1)

    def step(self, episode, observation, done):
        self.seq += 1
        joints, base = episode.pose()
        self.packet = dict(version=1, run_id=self.root.name, robot=self.task.config["robot"], source="training",
            phase="training", sequence=self.seq, wall_time=time.time(), sim_time=self.seq*self.task.dt,
            env_index=0, reset_count=self.resets, finished=False, command=episode.command.tolist(),
            dt=self.task.dt, joint_names=self.task.pose_names, joints=[joints], base=[base],
            observation=observation.tolist(), next_observation=episode.obs.tolist(),
            observation_names=[f"{o['name']}[{i}]" for o in self.task.config['observations'] if o['enabled'] for i in range(self.task.width(o['name']))],
            action=episode.action.tolist(), targets=episode.targets.tolist(),
            torques=episode.data.actuator_force[self.task.aids].tolist(), actuator_joints=self.task.names,
            reward_terms=episode.reward_terms, episode_step=episode.count, terminal=bool(done),
            sample_timing="observation before action; pose, next_observation and torque after control step")
        # Every control step is recorded; only viewport publication is throttled.
        self.log.write(json.dumps(self.packet, allow_nan=False)+"\n")
        if time.monotonic()-self.last >= .05:
            self.last = time.monotonic()
            atomic_json(self.root/"live-motion.json", self.packet)
        self.resets += int(done)

    def close(self):
        if self.packet:
            self.packet["finished"] = True
            atomic_json(self.root/"live-motion.json", self.packet)
        self.log.close()


class VecEnv:
    device = "cpu"

    def __init__(self, task, seed, motion):
        import torch
        self.task, self.motion, self.cfg = task, motion, task.config
        self.num_envs = self.cfg["ppo"]["num_envs"]
        self.num_actions = task.n_actions
        self.max_episode_length = max(1, round(self.cfg["simulation"]["episode_seconds"]/task.dt))
        self.episodes = [task.episode(seed+i) for i in range(self.num_envs)]
        self.episode_length_buf = torch.zeros(self.num_envs, dtype=torch.long)
        self.failures, self.lengths = deque(maxlen=100), deque(maxlen=100)
        self.rewards = []

    def get_observations(self):
        import torch
        from tensordict import TensorDict
        return TensorDict({"state": torch.from_numpy(np.stack([e.obs for e in self.episodes]))}, [self.num_envs])

    def step(self, actions):
        import torch
        before = self.episodes[0].obs.copy()
        results = [e.step(a) for e, a in zip(self.episodes, actions.detach().cpu().numpy())]
        dones = [r[2] or r[3] for r in results]
        self.motion.step(self.episodes[0], before, dones[0])
        self.episode_length_buf += 1
        self.rewards.extend(r[1] for r in results)
        for i, done in enumerate(dones):
            if done:
                self.failures.append(results[i][2])
                self.lengths.append(self.episodes[i].count)
                self.episodes[i].reset()
                self.episode_length_buf[i] = 0
        return (self.get_observations(), torch.tensor([r[1] for r in results], dtype=torch.float32),
                torch.tensor(dones, dtype=torch.long), {"time_outs": torch.tensor([r[3] for r in results], dtype=torch.float32)})


def checkpoint_policy(checkpoint, target):
    """Rebuild the exact normalized actor from a self-describing checkpoint."""
    import torch
    from tensordict import TensorDict
    from rsl_rl.models import MLPModel
    saved = torch.load(checkpoint, map_location="cpu", weights_only=True)
    info = saved.get("infos") or {}
    cfg = info["task_config"]
    validate_config(cfg)
    obs, actions = info["obs_size"], info["actions"]
    values = dict(runner_config(cfg, 0)["actor"]); values.pop("class_name")
    actor = MLPModel(TensorDict({"state": torch.zeros(1, obs)}, [1]), {"actor": ["state"]}, "actor", actions, **values)
    actor.load_state_dict(saved["actor_state_dict"])
    model = actor.as_jit().cpu().eval()
    torch.onnx.export(model, torch.zeros(1, obs), str(target), dynamo=False, input_names=["observation"],
                      output_names=["action"], opset_version=18)
    return model, obs, actions, None, "Nexus captured MuJoCo task / RSL-RL normalized actor"


def train(args):
    import torch
    from rsl_rl.runners import OnPolicyRunner
    cfg = json.loads(args.task_config.read_text())
    validate_config(cfg)
    p = cfg["ppo"]
    if importlib.metadata.version("rsl-rl-lib") != "5.0.1":
        raise RuntimeError("Generic task adapter requires rsl-rl-lib==5.0.1")
    if (p["device"] == "mps" and not torch.backends.mps.is_available()) or (p["device"] == "cuda" and not torch.cuda.is_available()):
        raise RuntimeError(f"Requested {p['device']} learner is unavailable; no fallback was started")
    torch.set_num_threads(4); torch.manual_seed(args.seed); np.random.seed(args.seed % 2**32)
    budget = args.iterations if args.iterations is not None else p["iterations"]
    if not 1 <= budget <= 1000000: raise ValueError("Invalid iteration budget")
    args.run_dir.mkdir(parents=True, exist_ok=True)
    training = args.run_dir/"training"; training.mkdir(exist_ok=False)
    resume, saved = None, None
    if args.resume_checkpoint:
        saved = torch.load(args.resume_checkpoint, weights_only=True, map_location="cpu")
        info = saved.get("infos") or {}
        # Find the source model next to the run, including checkpoints retained
        # under training/. Never reload a potentially edited library URDF.
        candidates = [args.resume_checkpoint.parent/"model.mjz", args.resume_checkpoint.parent.parent/"model.mjz"]
        snapshot = next((v for v in candidates if v.is_file() and sha(v) == info.get("model_sha256")), None)
        if snapshot is None: raise ValueError("Resume requires the original captured model.mjz with matching digest")
        task = Task(cfg, snapshot)
        if info.get("contract") != task.contract: raise ValueError("Resume checkpoint task contract differs from the captured recipe")
        inputs = args.run_dir/"input"; inputs.mkdir()
        resume = inputs/args.resume_checkpoint.name
        shutil.copyfile(args.resume_checkpoint, resume)
        shutil.copyfile(snapshot, args.run_dir/"model.mjz")
    else:
        task = Task(cfg)
        task.save_snapshot(args.run_dir/"model.mjz")
    snapshot_hash = sha(args.run_dir/"model.mjz")
    atomic_json(args.run_dir/"task.json", cfg)
    config = runner_config(cfg, args.seed)
    atomic_json(args.run_dir/"training-plan.json", dict(task_config=cfg, contract=task.contract, model_sha256=snapshot_hash,
        learner=p["device"], physics="cpu", num_envs=parallel.total_steps(p["num_envs"]), num_envs_per_process=p["num_envs"], processes=parallel.world(), iterations=budget,
        env_steps=parallel.total_steps(p["num_envs"]*budget*p["steps_per_env"]), runner=copy.deepcopy(config),
        reward_metric="mean weighted reward per control step", resume=str(resume) if resume else None))
    motion = Motion(args.run_dir, task)
    env = VecEnv(task, args.seed, motion)
    start_iteration = 0
    evaluated = set()

    class NexusRunner(parallel.RunnerMixin, OnPolicyRunner):
        def save(self, path, infos=None):
            checkpoint = Path(path)
            super().save(path, dict(robot=cfg["robot"], task_config=cfg, contract=task.contract,
                model_sha256=snapshot_hash, obs_size=task.obs_size, actions=task.n_actions,
                env_steps=parallel.total_steps(motion.seq*env.num_envs)))
            target = args.run_dir/"policy.pt"
            shutil.copyfile(checkpoint, target.with_suffix(".pt.tmp")); target.with_suffix(".pt.tmp").replace(target)
            iteration = self.current_learning_iteration+1
            emit("checkpoint", path=str(target), iteration=iteration)
            if iteration in evaluated: return
            evaluated.add(iteration)
            from export_checkpoint import verify
            from validate_generic import validate
            output = args.run_dir/"evaluations"/f"iteration_{iteration:06d}"
            out = output/"training"; out.mkdir(parents=True)
            model, obs, acts, _, _ = checkpoint_policy(checkpoint, out/"policy.onnx")
            maximum = verify(model, out/"policy.onnx", obs, acts)
            evidence = dict(robot=cfg["robot"], checkpoint=str(checkpoint.resolve()), checkpoint_sha256=sha(checkpoint),
                onnx_sha256=sha(out/"policy.onnx"), export_max_error=maximum, contract=task.contract,
                model_sha256=snapshot_hash, learner_device=p["device"], physics_device="cpu", iterations=iteration-start_iteration,
                total_iterations=iteration, env_steps=parallel.total_steps(motion.seq*env.num_envs),
                policy_parameter_device=str(next(self.alg.get_policy().parameters()).device),
                optimizer_states=len(self.alg.optimizer.state), observation_samples=float(self.alg.get_policy().obs_normalizer.count.item()))
            atomic_json(out/"verified.json", evidence)
            shutil.copyfile(out/"policy.onnx", args.run_dir/"policy.onnx")
            emit("status", message=f"Validating captured task checkpoint {iteration}")
            report = validate(task, out/"policy.onnx", output, args.run_dir)
            atomic_json(training/"verified.json", evidence)
            emit("validation", iteration=iteration, survived=report["survived"], total=report["total"],
                 tracking_passed=report["tracking_passed"], result=report["validation_gate"])

    try:
        runner = NexusRunner(env, config, str(training), parallel.device(p["device"]))
        actual = str(next(runner.alg.get_policy().parameters()).device)
        if not actual.startswith(p["device"]): raise RuntimeError("Policy device differs from requested learner")
        if resume:
            runner.load(str(resume), map_location="cpu")
            runner.current_learning_iteration += 1
            start_iteration = runner.current_learning_iteration
            runner.alg.learning_rate = float(runner.alg.optimizer.param_groups[0]["lr"])
            if not runner.alg.optimizer.state: raise RuntimeError("Optimizer state was not restored")
            emit("resumed", completed_iterations=start_iteration, optimizer_states=len(runner.alg.optimizer.state),
                 observation_samples=float(runner.alg.get_policy().obs_normalizer.count.item()))
        emit("device", learner=actual, physics="cpu")
        original_log = runner.logger.log

        def log(logger, **values):
            original_log(**values)
            emit("sample", step=motion.seq*env.num_envs, iteration=values["it"]-start_iteration+1,
                total_iteration=values["it"]+1, reward=statistics.mean(env.rewards),
                reward_metric="mean weighted reward per control step", completed_episodes=len(env.failures),
                fall_rate=statistics.mean(env.failures) if env.failures else 0.,
                fall_rate_measured=bool(env.failures),
                episode_len=statistics.mean(env.lengths) if env.lengths else float(env.episode_length_buf.float().mean()),
                iteration_env_steps=env.num_envs*p["steps_per_env"], iteration_seconds=values["collect_time"]+values["learn_time"],
                steps_per_sec=env.num_envs*p["steps_per_env"]/(values["collect_time"]+values["learn_time"]), learner_device=actual)
            env.rewards.clear()
        runner.logger.log = MethodType(log, runner.logger)
        runner.learn(budget)
        emit("complete", iterations=budget, total_iterations=runner.current_learning_iteration+1)
    finally:
        motion.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--task-config", type=Path, required=True)
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--iterations", type=int)
    parser.add_argument("--seed", type=lambda s: int(s, 16) if s.startswith("0x") else int(s), default=42)
    parser.add_argument("--resume-checkpoint", type=Path)
    args = parser.parse_args(); args.run_dir = args.run_dir.resolve(); parallel.prepare_args(args)
    try:
        with redirect_stdout(sys.stderr): train(args)
    except Exception as error:
        emit("error", message=str(error)); raise


if __name__ == "__main__": main()
