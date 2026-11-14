#!/usr/bin/env python3
"""Nexus adapter for the pinned official Microduck PPO task.

The supervisor uses only the standard library. The phase worker runs inside
the upstream uv environment on CUDA, CPU, or Apple MPS. No remote jobs are submitted.
"""
from __future__ import annotations

import argparse
from collections import deque
from contextlib import redirect_stdout
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import platform
import re
import shutil
import statistics
import subprocess
import sys
import time
from types import MethodType

COMMIT = "53b8971b61baf5b7f3c16d135dd7cac37623de4b"
TASK = "Mjlab-Velocity-Flat-MicroDuck"
EVENT_STREAM = sys.stdout


def emit(event, **fields):
    print(json.dumps({"event": event, **fields}, allow_nan=False), file=EVENT_STREAM, flush=True)


def atomic_json(path, value):
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")
    tmp.replace(path)


def hardware_check():
    if platform.system() != "Linux" or not shutil.which("nvidia-smi"):
        raise RuntimeError(
            "Microduck PPO requires a Linux NVIDIA CUDA training host. "
            "Use an MPS/CPU profile on Apple Silicon or configure a CUDA host; "
            "no training steps were executed by this CUDA profile."
        )
    result = subprocess.run(
        ["nvidia-smi", "--query-gpu=name,memory.total", "--format=csv,noheader"],
        text=True, capture_output=True, timeout=15, check=True,
    )
    if not result.stdout.strip():
        raise RuntimeError("No NVIDIA GPU was reported by nvidia-smi")
    return result.stdout.strip()


def check_source(repo):
    actual = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
    if actual != COMMIT:
        raise RuntimeError(f"Expected Microduck commit {COMMIT}, found {actual}")
    if subprocess.check_output(["git", "-C", str(repo), "status", "--porcelain", "--untracked-files=no"], text=True).strip():
        raise RuntimeError("The upstream Microduck checkout has changes; use the pinned clean recipe")


def hold_early_curriculum(cfg):
    """Delay curriculum hardening until the policy has learned to move."""
    held = {}
    for name, term in cfg.curriculum.items():
        for key, stages in list(term.params.items()):
            if key.endswith("_stages") and isinstance(stages, list) and stages:
                if stages[0].get("step") != 0:
                    raise RuntimeError(f"Unexpected initial curriculum stage: {name}.{key}")
                term.params[key] = stages[:1]
                held[name] = {key: stages[:1]}
    if len(held) != 7:
        raise RuntimeError(f"Expected seven Microduck flat-ground curricula, found {len(held)}")
    return held


def evaluate_saved_checkpoint(args, path, completed_iterations):
    output = args.run_dir / "evaluations" / f"iteration_{completed_iterations:06d}"
    output.mkdir(parents=True, exist_ok=False)
    emit("status", message=f"Validating checkpoint after {completed_iterations} total PPO iterations")
    with (output / "evaluation.log").open("w") as log:
        subprocess.run([
            sys.executable, "-u", str(Path(__file__).with_name("evaluate_microduck_checkpoint.py")),
            "--repo", str(args.repo), "--checkpoint", str(path), "--output", str(output),
        ], cwd=args.repo, stdout=log, stderr=log, check=True)
    report = json.loads((output / "validation/report.json").read_text())
    emit("validation", iteration=completed_iterations, survived=report["survived"],
         total=report["total"], tracking_passed=report["tracking_passed"],
         result=report["validation_gate"], report=str(output / "validation/report.md"))
    emit("status", message=f"Checkpoint validation: {report['tracking_passed']}/{report['total']} command-tracking trials passed; training continues")


def phase(args):
    """Run upstream run_train with logging hooks; PPO/physics/rewards stay upstream."""
    import numpy as np
    import torch

    if args.device == "cuda" and not torch.cuda.is_available():
        raise RuntimeError("PyTorch cannot access CUDA; verify the NVIDIA driver and upstream uv environment")
    if args.device == "mps" and not torch.backends.mps.is_available():
        raise RuntimeError("PyTorch MPS is unavailable; this profile requires an Apple GPU")
    os.environ["CUDA_VISIBLE_DEVICES"] = "" if args.device != "cuda" else os.environ.get("CUDA_VISIBLE_DEVICES", "0")
    if args.device == "cuda":
        os.environ.setdefault("MUJOCO_GL", "egl")
    else:
        torch.set_num_threads(4)
    with redirect_stdout(sys.stderr):
        import mjlab_microduck.tasks  # noqa: F401: registers the official tasks
        from mjlab.scripts import train
        from mjlab_microduck.export import ExportConfig, run_export
    for name, version in [("mjlab", "1.3.0"), ("rsl-rl-lib", "5.0.1")]:
        if importlib.metadata.version(name) != version:
            raise RuntimeError(f"Expected {name}=={version}; run uv sync --locked")

    is_smoke = args.phase == "smoke"
    output = args.run_dir / ("smoke" if is_smoke else "training")
    output.mkdir(parents=True, exist_ok=False)
    cfg = train.TrainConfig.from_task(TASK)
    cfg.env.scene.num_envs = 64 if is_smoke else args.num_envs
    cfg.agent.max_iterations = 5 if is_smoke else args.iterations
    cfg.agent.seed = args.seed
    cfg.agent.logger = "tensorboard"
    cfg.agent.upload_model = False
    cfg.agent.run_name = args.run_dir.name
    if args.evaluate_every:
        cfg.agent.save_interval = args.evaluate_every
    if args.hold_early_curriculum:
        held = hold_early_curriculum(cfg.env)
        atomic_json(output / "curriculum-override.json", held)
    if args.resume_checkpoint:
        # Preserve an immutable input in the new run. Upstream's resolver uses
        # <log-root>/<load_run>/<load_checkpoint>; no resolver monkeypatch needed.
        inputs = args.run_dir / "input"
        inputs.mkdir(exist_ok=True)
        checkpoint = inputs / args.resume_checkpoint.name
        digest = hashlib.sha256(args.resume_checkpoint.read_bytes()).hexdigest()
        if checkpoint.exists():
            if hashlib.sha256(checkpoint.read_bytes()).hexdigest() != digest:
                raise RuntimeError("Resume input changed between smoke and training")
        else:
            shutil.copyfile(args.resume_checkpoint, checkpoint)
        cfg.agent.resume = True
        cfg.agent.load_run = "input"
        cfg.agent.load_checkpoint = re.escape(checkpoint.name) + "$"
    if cfg.agent.num_steps_per_env != 24:
        raise RuntimeError("Upstream rollout horizon changed")
    original_loader = train.load_runner_cls
    base_runner = original_loader(TASK)
    last_checkpoint = None
    start_iteration = 0
    evaluated_iterations = set()
    motion = None

    class NexusRunner(base_runner):
        def __init__(self, env, train_cfg, log_dir=None, device="cpu", **kwargs):
            nonlocal motion
            learner_device = "mps" if args.device == "mps" else device
            super().__init__(env, train_cfg, log_dir, learner_device, **kwargs)
            policy_device = str(next(self.alg.get_policy().parameters()).device)
            if args.device == "mps" and not policy_device.startswith("mps"):
                raise RuntimeError("The Microduck policy was not allocated on the Apple GPU")
            emit("device", learner=policy_device, physics=str(env.device), phase=args.phase)
            obs = env.get_observations()
            if obs["actor"].shape[-1] != 61 or env.num_actions != 14:
                raise RuntimeError("Microduck policy contract must be 61 observations and 14 actions")
            logger = self.logger
            old_step, old_log = logger.process_env_step, logger.log
            failures = deque(maxlen=100)
            from microduck_motion import TrainingMotion
            motion = TrainingMotion(env, output if is_smoke and not args.smoke_only else args.run_dir, args.phase)
            motion.attach(env)

            def step_hook(_logger, rewards, dones, extras, intrinsic_rewards=None):
                # Episode terminations (falls/invalid states), excluding time limits.
                finished = dones > 0
                timeout = extras.get("time_outs")
                if timeout is None:
                    raise RuntimeError("Upstream trainer did not report episode timeouts")
                failures.extend((~timeout.to(finished.device).bool())[finished].cpu().tolist())
                old_step(rewards, dones, extras, intrinsic_rewards)

            def log_hook(_logger, **values):
                with redirect_stdout(sys.stderr):
                    old_log(**values)
                if not logger.rewbuffer or not failures:
                    return  # No completed episodes yet: do not invent measurements.
                emit("smoke_sample" if is_smoke and not args.smoke_only else "sample",
                     step=int(logger.tot_timesteps), iteration=int(values["it"]) - start_iteration + 1,
                     total_iteration=int(values["it"]) + 1,
                     reward=statistics.mean(logger.rewbuffer),
                     fall_rate=statistics.mean(failures),
                     steps_per_sec=logger.num_envs * 24 / (values["collect_time"] + values["learn_time"]),
                     episode_len=statistics.mean(logger.lenbuffer), learner_device=policy_device,
                     mps_allocated_bytes=torch.mps.current_allocated_memory() if args.device == "mps" else 0)

            logger.process_env_step = MethodType(step_hook, logger)
            logger.log = MethodType(log_hook, logger)

        def load(self, path, load_cfg=None, strict=True, map_location=None):
            nonlocal start_iteration
            # load_state_dict moves network/optimizer tensors to the learner;
            # keep Adam's non-capturable step counters on CPU.
            result = super().load(path, load_cfg, strict, map_location="cpu")
            self.current_learning_iteration += 1  # saved index was already completed
            start_iteration = self.current_learning_iteration
            if not self.alg.optimizer.state:
                raise RuntimeError("Resume did not restore PPO optimizer state")
            self.alg.learning_rate = float(self.alg.optimizer.param_groups[0]["lr"])
            emit("resumed", phase=args.phase, checkpoint=str(path),
                 source_sha256=hashlib.sha256(Path(path).read_bytes()).hexdigest(),
                 completed_iterations=start_iteration,
                 curriculum_step=int(self.env.unwrapped.common_step_counter),
                 learner=str(next(self.alg.get_policy().parameters()).device),
                 optimizer_states=len(self.alg.optimizer.state),
                 learning_rate=self.alg.learning_rate,
                 observation_samples=float(self.alg.get_policy().obs_normalizer.count.item()))
            return result

        def save(self, path, infos=None):
            nonlocal last_checkpoint
            with redirect_stdout(sys.stderr):
                super().save(path, infos)
            last_checkpoint = Path(path)
            target = output / "policy.pt" if is_smoke and not args.smoke_only else args.run_dir / "policy.pt"
            temp = target.with_suffix(".pt.tmp")
            shutil.copyfile(path, temp)
            temp.replace(target)
            emit("checkpoint", phase=args.phase, path=str(target), iteration=self.current_learning_iteration)
            if not is_smoke and args.evaluate_every and self.current_learning_iteration not in evaluated_iterations:
                # Use the upstream save cadence, including the first resumed
                # checkpoint and final save, so the evaluation pipeline is
                # exercised before committing hours to the new stage.
                evaluated_iterations.add(self.current_learning_iteration)
                evaluate_saved_checkpoint(args, last_checkpoint, self.current_learning_iteration + 1)

    train.load_runner_cls = lambda task: NexusRunner if task == TASK else base_runner
    try:
        with redirect_stdout(sys.stderr):
            train.run_train(TASK, cfg, output)
    finally:
        train.load_runner_cls = original_loader
        if motion is not None:
            motion.close()
    if last_checkpoint is None or not last_checkpoint.is_file():
        raise RuntimeError("Upstream training produced no checkpoint")
    onnx_path = (output if is_smoke and not args.smoke_only else args.run_dir) / "policy.onnx"
    with redirect_stdout(sys.stderr):
        run_export(TASK, ExportConfig(checkpoint_file=str(last_checkpoint),
                                     onnx_file=str(onnx_path), num_envs=1,
                                     device="cuda:0" if args.device == "cuda" else "cpu"))
    import onnxruntime as ort
    session = ort.InferenceSession(str(onnx_path), providers=["CPUExecutionProvider"])
    if session.get_inputs()[0].shape != [1, 61] or session.get_outputs()[0].shape != [1, 14]:
        raise RuntimeError("Exported ONNX has an incompatible policy shape")
    for obs in [np.zeros((1, 61), dtype=np.float32), np.random.default_rng(args.seed).normal(0, 0.1, (1, 61)).astype(np.float32)]:
        if not np.isfinite(session.run(None, {session.get_inputs()[0].name: obs})[0]).all():
            raise RuntimeError("Exported ONNX produced non-finite actions")
    evidence = {"task": TASK, "commit": COMMIT, "phase": args.phase,
                "learner_device": args.device, "physics_device": "cuda" if args.device == "cuda" else "cpu",
                "iterations": cfg.agent.max_iterations, "num_envs": cfg.env.scene.num_envs,
                "env_steps": cfg.agent.max_iterations * cfg.env.scene.num_envs * 24,
                "prior_iterations": start_iteration,
                "total_iterations": start_iteration + cfg.agent.max_iterations,
                "resume_checkpoint": str(args.resume_checkpoint) if args.resume_checkpoint else None,
                "hold_early_curriculum": args.hold_early_curriculum,
                "onnx": str(onnx_path), "checkpoint": str(last_checkpoint),
                "onnx_sha256": hashlib.sha256(onnx_path.read_bytes()).hexdigest(),
                "onnx_shape": "[1,61] -> [1,14]", "normalizer": "upstream export.py",
                "completed_at": time.time()}
    atomic_json(output / "verified.json", evidence)
    emit("phase_complete", **evidence)


def supervise(args):
    args.run_dir.mkdir(parents=True, exist_ok=True)
    check_source(args.repo)
    plan = {"task": TASK, "commit": COMMIT, "robot": "Microduck", "backend": "mjlab PPO / MuJoCo Warp",
            "smoke": {"num_envs": 64, "iterations": 5, "env_steps": 7680},
            "training": {"num_envs": args.num_envs, "iterations": args.iterations,
                         "env_steps": args.num_envs * args.iterations * 24},
            "smoke_only": args.smoke_only,
            "resume_checkpoint": str(args.resume_checkpoint) if args.resume_checkpoint else None,
            "resume_sha256": hashlib.sha256(args.resume_checkpoint.read_bytes()).hexdigest() if args.resume_checkpoint else None,
            "hold_early_curriculum": args.hold_early_curriculum,
            "evaluate_every": args.evaluate_every,
            "seed": args.seed, "checkpoint": str(args.run_dir / "policy.pt"),
            "onnx": str(args.run_dir / "policy.onnx"),
            "recipe": "Upstream task with all seven curricula held at their initial stages." if args.hold_early_curriculum else "Upstream task configuration; Nexus G1 scene sliders do not modify this task."}
    atomic_json(args.run_dir / "microduck-plan.json", plan)
    if args.plan_only:
        print(json.dumps(plan, indent=2))
        return
    if args.device != "cuda":
        plan["device"] = args.device.upper()
        plan["physics_device"] = "CPU"
    else:
        plan["gpu"] = hardware_check()
    atomic_json(args.run_dir / "microduck-plan.json", plan)
    uv = shutil.which("uv")
    if not uv:
        raise RuntimeError("Install uv on the CUDA training host before starting this profile")
    subprocess.run([uv, "sync", "--locked", "--python", "3.12"], cwd=args.repo,
                   stdout=sys.stderr, stderr=sys.stderr, check=True)
    for part in (["smoke"] if args.smoke_only else ["smoke", "train"]):
        emit("status", message=f"Microduck {part}: {'64 environments / 5 iterations' if part == 'smoke' else str(args.num_envs) + ' environments / ' + str(args.iterations) + ' iterations'}")
        cmd = [uv, "run", "--no-sync", "python", "-u", str(Path(__file__).resolve()),
               "--repo", str(args.repo), "--run-dir", str(args.run_dir), "--seed", str(args.seed),
               "--num-envs", str(args.num_envs), "--iterations", str(args.iterations),
               "--device", args.device, "--phase", part]
        if args.smoke_only:
            cmd.append("--smoke-only")
        if args.resume_checkpoint:
            cmd += ["--resume-checkpoint", str(args.resume_checkpoint)]
        if args.hold_early_curriculum:
            cmd.append("--hold-early-curriculum")
        if args.evaluate_every:
            cmd += ["--evaluate-every", str(args.evaluate_every)]
        subprocess.run(cmd, cwd=args.repo, check=True)
        if not (args.run_dir / ("smoke" if part == "smoke" else "training") / "verified.json").is_file():
            raise RuntimeError(f"Microduck {part} did not produce verified export evidence")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--seed", type=lambda s: int(s, 0), default=0xC0FFEE)
    parser.add_argument("--num-envs", type=int, default=4096)
    parser.add_argument("--iterations", type=int, default=6000)
    parser.add_argument("--phase", choices=["smoke", "train"])
    parser.add_argument("--device", choices=["cuda", "cpu", "mps"], default="cuda")
    parser.add_argument("--smoke-only", action="store_true")
    parser.add_argument("--plan-only", action="store_true")
    parser.add_argument("--resume-checkpoint", type=Path)
    parser.add_argument("--hold-early-curriculum", action="store_true")
    parser.add_argument("--evaluate-every", type=int, default=0)
    args = parser.parse_args()
    args.repo, args.run_dir = args.repo.resolve(), args.run_dir.resolve()
    if args.resume_checkpoint:
        args.resume_checkpoint = args.resume_checkpoint.resolve(strict=True)
    try:
        if args.num_envs < 4 or args.iterations < 1:
            raise ValueError("Training requires at least 4 environments and 1 iteration")
        if args.evaluate_every < 0:
            raise ValueError("Evaluation interval cannot be negative")
        if args.phase:
            phase(args)
        else:
            supervise(args)
    except Exception as exc:
        message = f"Microduck training could not finish: {exc}"
        emit("error", message=message)
        print(message, file=sys.stderr, flush=True)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
