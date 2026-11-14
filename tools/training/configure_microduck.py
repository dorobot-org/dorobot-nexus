#!/usr/bin/env python3
"""Create a Nexus profile for the pinned official Microduck trainer."""
import argparse
import json
import os
from pathlib import Path
import sys

from microduck_worker import TASK, check_source


def main():
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=root / "training-stack/microduck_rl")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--python", default=sys.executable)
    parser.add_argument("--num-envs", type=int, default=4096)
    parser.add_argument("--iterations", type=int, default=6000)
    parser.add_argument("--cpu-smoke", action="store_true")
    parser.add_argument("--device", choices=["cuda", "cpu", "mps"], default="cuda")
    parser.add_argument("--resume-checkpoint", type=Path)
    parser.add_argument("--hold-early-curriculum", action="store_true")
    parser.add_argument("--evaluate-every", type=int, default=0)
    args = parser.parse_args()
    check_source(args.repo)
    if args.num_envs < 4 or args.iterations < 1:
        parser.error("Use at least 4 environments and 1 iteration")
    if args.evaluate_every < 0:
        parser.error("Evaluation interval cannot be negative")
    name = "microduck.cpu-smoke.profile.json" if args.cpu_smoke else f"microduck.{args.device}.profile.json"
    output = (args.output or root / "training-stack" / name).resolve()
    command = ["-u", str(root / "tools/training/microduck_worker.py"), "--repo", str(args.repo.resolve()),
               "--run-dir", "{run_dir}", "--seed", "{seed}", "--num-envs", "{num_envs}",
               "--iterations", "{iterations}"]
    if args.cpu_smoke:
        command += ["--device", "cpu", "--smoke-only"]
    else:
        command += ["--device", args.device]
    if args.resume_checkpoint:
        command += ["--resume-checkpoint", str(args.resume_checkpoint.resolve(strict=True))]
    if args.hold_early_curriculum:
        command.append("--hold-early-curriculum")
    if args.evaluate_every:
        command += ["--evaluate-every", str(args.evaluate_every)]
    profile = {"robot": "Microduck", "program": args.python, "args": command, "cwd": str(root),
               "checkpoint": "{run_dir}/policy.pt", "env": {
                   "PYTHONUNBUFFERED": "1", "UV_HTTP_TIMEOUT": "600",
                   "PATH": str(root / "training-stack/bin") + os.pathsep + os.environ.get("PATH", "")},
               "recipe": {"task": TASK + (" · CPU smoke test" if args.cpu_smoke else " · " + args.device.upper()),
                          "num_envs": 64 if args.cpu_smoke else args.num_envs,
                          "iterations": 5 if args.cpu_smoke else args.iterations}}
    if args.resume_checkpoint:
        profile["resume_checkpoint"] = str(args.resume_checkpoint.resolve(strict=True))
        profile["recipe"]["task"] += " · continued"
    if args.hold_early_curriculum:
        profile["recipe"]["task"] += " · learn walking"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(profile, indent=2) + "\n")
    print(output)


if __name__ == "__main__":
    main()
