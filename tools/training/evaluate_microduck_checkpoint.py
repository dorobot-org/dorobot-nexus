#!/usr/bin/env python3
"""Export and evaluate one immutable Microduck checkpoint, isolated from PPO."""
import argparse
from contextlib import redirect_stdout
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

from microduck_worker import COMMIT, TASK, atomic_json, check_source


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--render", action="store_true")
    args = parser.parse_args()
    repo = args.repo.resolve()
    checkpoint = args.checkpoint.resolve(strict=True)
    output = args.output.resolve()
    check_source(repo)
    output.mkdir(parents=True, exist_ok=True)
    os.environ["CUDA_VISIBLE_DEVICES"] = ""
    os.chdir(repo)
    import torch
    torch.set_num_threads(2)
    with redirect_stdout(sys.stderr):
        import mjlab_microduck.tasks  # noqa: F401
        from mjlab_microduck.export import ExportConfig, run_export
        run_export(TASK, ExportConfig(checkpoint_file=str(checkpoint),
                                     onnx_file=str(output / "policy.onnx"),
                                     num_envs=1, device="cpu"))
    evidence = {"task": TASK, "commit": COMMIT, "checkpoint": str(checkpoint),
                "checkpoint_sha256": hashlib.sha256(checkpoint.read_bytes()).hexdigest(),
                "onnx_sha256": hashlib.sha256((output / "policy.onnx").read_bytes()).hexdigest(),
                "normalizer": "upstream export.py", "purpose": "checkpoint evaluation"}
    (output / "training").mkdir(exist_ok=True)
    atomic_json(output / "training/verified.json", evidence)
    command = [sys.executable, "-u", str(Path(__file__).with_name("validate_microduck.py")),
               "--repo", str(repo), "--run-dir", str(output)]
    if args.render:
        command.append("--render")
    subprocess.run(command, check=True)
    report = json.loads((output / "validation/report.json").read_text())
    if report["status"] != "completed" or report["total"] != 15:
        raise RuntimeError("Checkpoint evaluation did not complete all 15 trials")
    from microduck_motion import publish_validation
    if output.is_relative_to(checkpoint.parent.parent):
        publish_validation(checkpoint.parent.parent, output)


if __name__ == "__main__":
    main()
