#!/usr/bin/env python3
"""Measured deterministic Open Duck checkpoint rollouts, independent of training resets."""
import argparse
import hashlib
import json
from pathlib import Path

import numpy as np
import onnxruntime as ort

from microduck_motion import atomic_json, publish_validation
from open_duck_env import DT, LOCKED_JOINTS, ROBOT, DuckTask

COMMANDS = [("stand", [0, 0, 0]), ("forward_slow", [0.05, 0, 0]),
            ("forward", [0.15, 0, 0]), ("turn_left", [0, 0, 0.5]), ("turn_right", [0, 0, -0.5])]


def validate(task, policy, output, run_dir, seconds=10.0):
    dt=task.dt
    session = ort.InferenceSession(str(policy), providers=["CPUExecutionProvider"])
    output = Path(output).resolve()
    directory = output / "validation"
    directory.mkdir(parents=True, exist_ok=True)
    cases = []
    for name, command in COMMANDS:
        for seed in range(3):
            episode = task.episode(seed, training=False)
            episode.reset(command)
            joints, bases = [], []
            velocities, tilts = [], []
            first_fall = None
            for frame in range(round(seconds / dt) + 1):
                joint, base = episode.pose()
                joints.append(joint)
                bases.append(base)
                tilt = float(np.degrees(np.arccos(np.clip(episode.sensor("upvector")[-1], -1, 1))))
                tilts.append(tilt)
                velocity = [*episode.sensor("local_linvel")[:2], episode.sensor("gyro")[2]]
                if first_fall is None:
                    velocities.append(velocity)
                # Stricter validation than the upstream upside-down training termination.
                if (tilt > 60 or base[2] < 0.07) and first_fall is None:
                    first_fall = frame * dt
                if first_fall is not None or frame == round(seconds / dt):
                    break
                action = session.run(None, {session.get_inputs()[0].name: episode.obs[None]})[0][0]
                episode.step(action)
            velocity = np.asarray(velocities)
            rmse = np.sqrt(np.mean((velocity - command) ** 2, axis=0))
            passed = first_fall is None and bool(np.all(rmse <= [0.05, 0.05, 0.2]))
            rollout = directory / f"{name}_seed{seed}.json"
            atomic_json(rollout, dict(name=f"{ROBOT} {name} seed {seed}", dt=dt,
                        joint_names=task.names + list(LOCKED_JOINTS), joints=joints, base=bases,
                        locked_joints=LOCKED_JOINTS))
            cases.append(dict(case=name, seed=seed, command=command, survived=first_fall is None,
                              tracking_pass=passed, max_tilt_deg=max(tilts), first_fall_seconds=first_fall,
                              recorded_seconds=(len(joints)-1)*dt,
                              upright_mean_velocity=velocity.mean(axis=0).tolist(), velocity_rmse=rmse.tolist(),
                              rollout=str(rollout)))
    report = dict(status="completed", robot=ROBOT, controller="Open Duck position actuators with captured calibration and limits", task_config=task.config,
                  policy_sha256=hashlib.sha256(Path(policy).read_bytes()).hexdigest(),
                  survived=sum(c["survived"] for c in cases), total=len(cases),
                  tracking_passed=sum(c["tracking_pass"] for c in cases), cases=cases,
                  duration_seconds=seconds, thresholds=dict(max_tilt_deg=60, min_root_height=0.07,
                                                           linear_rmse=0.05, yaw_rmse=0.2))
    report["validation_gate"] = "PASS" if report["tracking_passed"] == len(cases) else "FAIL"
    atomic_json(directory / "report.json", report)
    lines = [f"# {ROBOT} checkpoint validation", "", f"Result: **{report['validation_gate']}**. "
             f"Survival {report['survived']}/{len(cases)}; command tracking {report['tracking_passed']}/{len(cases)}.", "",
             "Deterministic exported ONNX policy, 10-second MuJoCo trials, three seeded home-pose perturbations per command. "
             "Thresholds: tilt ≤60°, root height ≥0.07 m, per-axis linear velocity RMSE ≤0.05 m/s, yaw RMSE ≤0.20 rad/s. "
             "A trial ends and fails at its first fall; the replay contains only measured frames up to that point. "
             "These checks validate the Nexus flat-terrain port, not hardware readiness.", "",
             "| Command | Seed | Survived | Tracking | Mean vx / vy / yaw |", "|---|---:|---|---|---|"]
    lines += [f"| {c['case']} | {c['seed']} | {c['survived']} | {c['tracking_pass']} | "
              + " / ".join(f"{v:.3f}" for v in c["upright_mean_velocity"]) + " |" for c in cases]
    (directory / "report.md").write_text("\n".join(lines) + "\n")
    publish_validation(run_dir, output, robot=ROBOT)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--evaluation-dir", type=Path, required=True)
    parser.add_argument("--run-dir", type=Path, required=True)
    args = parser.parse_args()
    report = validate(DuckTask(args.repo), args.evaluation_dir / "training/policy.onnx", args.evaluation_dir, args.run_dir)
    print(json.dumps({k: report[k] for k in ("validation_gate", "survived", "total", "tracking_passed")}))
