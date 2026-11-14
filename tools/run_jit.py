#!/usr/bin/env python3
"""Run a Studio through the pinned Makepad Relax Rust JIT."""
import fcntl
import json
import os
from pathlib import Path
import subprocess
import sys

PROJECT = Path(__file__).resolve().parents[1]
CONFIG = json.loads((PROJECT / "jit-runtime.json").read_text())


def runner():
    override = os.environ.get("DOROBOT_JIT_RUNNER")
    if override:
        binary = Path(override).expanduser().resolve()
        if not binary.is_file():
            raise RuntimeError("DOROBOT_JIT_RUNNER must name the Relax executable")
        return binary
    cache = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    cache = (cache / "dorobot" / "makepad-jit" / CONFIG["revision"]).resolve()
    target = cache / "target"
    binary = target / "release" / "relax"
    source_override = os.environ.get("MAKEPAD_JIT_SOURCE")
    cache.mkdir(parents=True, exist_ok=True)
    # Both Studios share this cache. Serialize first-run fetch/build and retry
    # an interrupted checkout instead of mistaking its directory for a build.
    with (cache / "setup.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if binary.is_file() and not source_override:
            return binary
        source = Path(source_override).expanduser().resolve() if source_override else cache / "source"
        if not source_override:
            source.mkdir(parents=True, exist_ok=True)
            subprocess.run(["git", "init", "--quiet", str(source)], check=True)
            head = subprocess.run(["git", "-C", str(source), "rev-parse", "--verify", "HEAD"],
                                  text=True, capture_output=True)
            if head.returncode or head.stdout.strip() != CONFIG["revision"]:
                subprocess.run(["git", "-C", str(source), "fetch", "--depth=1",
                                CONFIG["repository"], CONFIG["revision"]], check=True)
                subprocess.run(["git", "-C", str(source), "checkout", "--detach", "--quiet", "FETCH_HEAD"], check=True)
        revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
        if revision != CONFIG["revision"]:
            raise RuntimeError("MAKEPAD_JIT_SOURCE must be at the revision in jit-runtime.json")
        if not binary.is_file():
            print("Building the pinned Makepad JIT runner (first launch only).", flush=True)
            env = dict(os.environ, CARGO_TARGET_DIR=str(target))
            subprocess.run(["cargo", "build", "--locked", "--release", "-p", "relax-runner", "--bin", "relax"], cwd=source, env=env, check=True)
        return binary



def main():
    args = sys.argv[1:]
    if args and args[0] in ("-h", "--help"):
        print("Usage: tools/run_jit.sh [--check | --setup | --nano] [--video] [application arguments]")
        print("Builds the pinned runner once, then JIT-compiles the Studio from source.")
        print("DOROBOT_JIT_RUNNER uses an existing runner; MAKEPAD_JIT_SOURCE reuses a pinned source checkout.")
        return
    mode = "run"
    if args and args[0] in ("--check", "--setup", "--nano"):
        mode = args.pop(0)[2:]
    video = "--video" in args
    if video:
        args.remove("--video")
        if not CONFIG.get("optional_video"):
            raise RuntimeError("--video is only supported by DoRobot Studio")
    binary = runner()
    if mode == "setup":
        print("Makepad JIT runner is ready.")
        return
    os.environ["MAKEPAD_RUN_ALLOW_C"] = "1"
    if mode == "nano":
        if CONFIG.get("optional_video"):
            dataset = PROJECT / "dataset" / "adora-nano-synthetic-preview"
            if not (dataset / "meta" / "info.json").is_file():
                subprocess.run([str(binary), "run", "--allow-c", CONFIG["package"],
                                "--no-default-features", "--example", "nano_preview",
                                "--", str(dataset)], cwd=PROJECT, check=True)
            args = ["--dataset", str(dataset), "--screen", "play", *args]
        else:
            args = ["--robot", "data/adora_nano/adora_nano.urdf", *args]
        mode = "run"
    command = [str(binary), mode, "--allow-c", CONFIG["package"], "--bin", CONFIG["binary"]]
    if CONFIG.get("optional_video") and not video:
        command.append("--no-default-features")
    if mode == "run":
        command.extend(["--", *args])
    elif args:
        raise RuntimeError("--check does not take application arguments")
    os.chdir(PROJECT)
    os.execv(binary, command)


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print("Studio JIT: " + str(error), file=sys.stderr)
        sys.exit(1)
