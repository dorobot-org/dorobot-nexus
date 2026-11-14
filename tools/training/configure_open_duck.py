#!/usr/bin/env python3
"""Create an Open Duck profile and register it alongside existing robot profiles."""
import argparse
import json
from pathlib import Path


def main():
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=root / "training-stack/open_duck_playground")
    parser.add_argument("--python", default=str(root / "training-stack/microduck_rl/.venv/bin/python"))
    parser.add_argument("--output", type=Path, default=root / "training-stack/open-duck.mps.profile.json")
    parser.add_argument("--registry", type=Path, default=root / "training-stack/robots.profiles.json")
    parser.add_argument("--microduck-profile", type=Path)
    parser.add_argument("--num-envs", type=int, default=32)
    parser.add_argument("--iterations", type=int, default=1000)
    parser.add_argument("--evaluate-every", type=int, default=100)
    parser.add_argument("--device", choices=["mps", "cpu"], default="mps")
    parser.add_argument("--resume-checkpoint", type=Path)
    args = parser.parse_args()
    if args.num_envs < 4 or args.iterations < 1 or args.evaluate_every < 1:
        parser.error("Use at least 4 environments and positive budgets")
    command = ["-u", str(root / "tools/training/open_duck_worker.py"), "--repo", str(args.repo.resolve()),
               "--run-dir", "{run_dir}", "--seed", "{seed}", "--num-envs", "{num_envs}",
               "--iterations", "{iterations}", "--device", args.device,
               "--evaluate-every", str(args.evaluate_every)]
    profile = dict(robot="Open Duck Mini v2", program=str(Path(args.python).absolute()), args=command, cwd=str(root),
                   checkpoint="{run_dir}/policy.pt", env={"PYTHONUNBUFFERED": "1"},
                   recipe=dict(task=f"Open Duck Mini v2 · flat terrain · {args.device.upper()} PPO",
                               num_envs=args.num_envs, iterations=args.iterations))
    if args.resume_checkpoint:
        checkpoint = str(args.resume_checkpoint.resolve(strict=True))
        command += ["--resume-checkpoint", checkpoint]
        profile["resume_checkpoint"] = checkpoint
        profile["recipe"]["task"] += " · continued"
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(profile, indent=2) + "\n")
    registry = json.loads(args.registry.read_text()) if args.registry.is_file() else {"profiles": {}}
    registry["profiles"]["Open Duck Mini v2"] = str(args.output.resolve())
    if args.microduck_profile:
        micro = args.microduck_profile.resolve(strict=True)
        if json.loads(micro.read_text()).get("robot") != "Microduck":
            parser.error("Microduck profile must identify robot Microduck")
        registry["profiles"]["Microduck"] = str(micro)
    args.registry.parent.mkdir(parents=True, exist_ok=True)
    args.registry.write_text(json.dumps(registry, indent=2) + "\n")
    print(f"NEXUS_TRAINING_PROFILE={args.registry.resolve()}")


if __name__ == "__main__":
    main()
