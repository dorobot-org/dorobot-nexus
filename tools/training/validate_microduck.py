#!/usr/bin/env python3
"""Bounded ONNX deployment rehearsal using the pinned upstream BAM controller.

Run with training-stack/microduck_rl/.venv/bin/python. Outputs measured results,
Nexus-compatible replay JSONs, and optional videos; never commands hardware.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import subprocess

import mujoco
import numpy as np

COMMIT = "53b8971b61baf5b7f3c16d135dd7cac37623de4b"
DT = 0.02
# Matches this run's saved env.yaml: terminations.fell_over.limit_angle.
FALL_ANGLE = 70.0
COMMANDS = {
    "stand": (0.0, 0.0, 0.0),
    "forward_slow": (0.1, 0.0, 0.0),
    "forward": (0.2, 0.0, 0.0),
    "turn_left": (0.0, 0.0, 0.5),
    "turn_right": (0.0, 0.0, -0.5),
}


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")


def rehearse(ip, policy_path, out, name, command, seed, seconds, render):
    bam = ip.load_bam_model(ip.BAM_KP_FW, 7.4, ip.BAM_MAX_CURRENT)
    model, data, ctrl, _ = ip.load_mujoco_with_bam(
        str(Path(ip.__file__).resolve().parents[1] / ip.MICRODUCK_XML),
        bam, 0.005, 0.1, ip.BAM_VIN_MIN,
    )
    policy = ip.PolicyInference(
        model, data, walking_onnx_path=str(policy_path), bam_ctrl=ctrl,
        use_projected_gravity=True, new_cmd_obs=True, action_scale=1.0,
    )
    policy.set_vel_cmd(*command)
    assert policy.walking_session.get_inputs()[0].shape == [1, 61]
    assert policy.walking_session.get_outputs()[0].shape == [1, 14]
    free = model.joint("trunk_base_freejoint")
    qa, va = int(free.qposadr[0]), int(free.dofadr[0])
    mujoco.mj_resetData(model, data)
    data.qpos[qa:qa + 7] = [0, 0, 0.125, 1, 0, 0, 0]
    data.qpos[policy.joint_qpos_indices] = policy.default_pose
    # Seed zero is the exact upstream rehearsal spawn. Others are independent
    # small initial pose perturbations, not full training domain randomization.
    if seed:
        rng = np.random.default_rng(seed)
        data.qpos[qa + 2] += rng.uniform(-0.001, 0.001)
        roll, pitch = rng.uniform(-math.radians(2), math.radians(2), 2)
        data.qpos[qa + 3:qa + 7] = [
            math.cos(roll / 2) * math.cos(pitch / 2),
            math.sin(roll / 2) * math.cos(pitch / 2),
            math.cos(roll / 2) * math.sin(pitch / 2),
            -math.sin(roll / 2) * math.sin(pitch / 2),
        ]
        data.qpos[policy.joint_qpos_indices] += rng.uniform(-0.01, 0.01, model.nu)
    ctrl.reset(data.qpos)
    policy.set_position_targets(policy.default_pose)
    mujoco.mj_forward(model, data)
    assert policy.get_observations().shape == (61,)
    np.testing.assert_allclose(policy.get_observations()[48:51], command)
    np.testing.assert_array_equal(policy.get_observations()[51:], np.zeros(10))
    joint_names = [model.joint(int(j)).name for j in model.actuator_trnid[:, 0]]
    replay = {"name": name, "dt": DT, "joint_names": joint_names,
              "joints": [], "base": [], "resets": []}
    states, velocities = [], []
    first_fall = None
    finite = True
    warning_counts = data.warning.number.copy()
    for step in range(round(seconds / DT) + 1):
        # mj_forward refreshes xquat/sensors for the completed integration step.
        mujoco.mj_forward(model, data)
        quat = data.qpos[qa + 3:qa + 7].copy()
        tilt = math.degrees(math.acos(float(np.clip(
            -policy.get_projected_gravity()[2], -1, 1))))
        xyz = data.qpos[qa:qa + 3].copy()
        replay["joints"].append(data.qpos[policy.joint_qpos_indices].tolist())
        replay["base"].append([*xyz.tolist(), *quat[1:].tolist(), float(quat[0])])
        states.append({"time_s": step * DT, "height_m": float(xyz[2]),
                       "tilt_deg": tilt, "qpos": data.qpos.tolist()})
        if tilt > FALL_ANGLE and first_fall is None:
            first_fall = step * DT
        if first_fall is None:
            v_body = policy.quat_rotate_inverse(quat, data.qvel[va:va + 3])
            velocities.append([float(v_body[0]), float(v_body[1]), float(data.qvel[va + 5])])
        # Preserve half a second after the failure for visual inspection.
        if step == round(seconds / DT) or (first_fall is not None and step * DT >= first_fall + 0.5):
            break
        action = policy.infer()
        if not np.isfinite(action).all():
            finite = False
            break
        policy.apply_action(action)
        for _ in range(4):
            ctrl.update()
            mujoco.mj_step(model, data)
        if not np.isfinite(data.qpos).all() or not np.isfinite(data.qvel).all():
            finite = False
            break
    warnings = (data.warning.number - warning_counts).tolist()
    # MuJoCo can recover from numerical divergence by resetting internally;
    # such a rollout must not pass just because the recovered state is finite.
    numerics_ok = finite and not any(warnings)
    prefix = f"{name}_seed{seed}"
    write_json(out / f"{prefix}.rollout.json", replay)
    write_json(out / f"{prefix}.states.json", states)
    measured = np.array(velocities)
    # Track only the upright portion; include this window explicitly so short
    # pre-fall transients cannot be mistaken for steady-state walking speed.
    rmse = np.sqrt(np.mean((measured - command) ** 2, axis=0)).tolist() if len(measured) else None
    result = {
        "case": name, "seed": seed, "command": list(command),
        "horizon_s": seconds, "recorded_s": states[-1]["time_s"],
        "first_fall_s": first_fall, "survived": first_fall is None and numerics_ok,
        "finite": finite, "mujoco_warning_counts": warnings,
        "min_height_m": min(s["height_m"] for s in states),
        "max_tilt_deg": max(s["tilt_deg"] for s in states),
        "upright_velocity_samples": len(velocities),
        "upright_mean_velocity": measured.mean(axis=0).tolist() if len(measured) else None,
        "upright_velocity_rmse": rmse,
        "rollout": str(out / f"{prefix}.rollout.json"),
    }
    if render:
        result.update(render_rollout(model, data, states, out, prefix, command))
    return result


def render_rollout(model, data, states, out, prefix, command):
    import imageio.v2 as imageio
    from PIL import Image, ImageDraw

    renderer = mujoco.Renderer(model, height=480, width=640)
    camera = mujoco.MjvCamera()
    camera.type = mujoco.mjtCamera.mjCAMERA_FREE
    camera.azimuth = 135
    camera.elevation = -12
    camera.distance = 0.7
    video = out / f"{prefix}.mp4"
    picks = set(np.linspace(0, len(states) - 1, 6).round().astype(int).tolist())
    stills = []
    try:
        with imageio.get_writer(video, fps=50, codec="libx264", quality=8) as writer:
            for i, state in enumerate(states):
                data.qpos[:] = state["qpos"]
                mujoco.mj_forward(model, data)
                camera.lookat[:] = [data.qpos[0], data.qpos[1], 0.11]
                renderer.update_scene(data, camera=camera)
                frame = Image.fromarray(renderer.render())
                draw = ImageDraw.Draw(frame)
                draw.rectangle((0, 0, 640, 42), fill="black")
                draw.text((10, 6), f"{prefix}  command vx,vy,wz={command}", fill="white")
                draw.text((10, 23), f"t={state['time_s']:.2f}s   tilt={state['tilt_deg']:.1f} deg   trunk z={state['height_m']:.3f}m", fill="white")
                writer.append_data(np.asarray(frame))
                if i in picks:
                    stills.append(frame.copy())
    finally:
        renderer.close()
    sheet = Image.new("RGB", (640 * 3, 480 * 2), "white")
    for i, frame in enumerate(stills):
        sheet.paste(frame, ((i % 3) * 640, (i // 3) * 480))
    sheet_path = out / f"{prefix}.png"
    sheet.save(sheet_path)
    return {"video": str(video), "contact_sheet": str(sheet_path)}


def summarize(report, out):
    report["survived"] = sum(c["survived"] for c in report["cases"])
    report["total"] = len(report["cases"])
    report["status"] = "completed"
    report["survival_gate"] = "pass" if report["survived"] == report["total"] else "fail"
    # A separate local acceptance check prevents standing still from passing a
    # locomotion test. These tolerances are explicit, not upstream certification.
    tolerances = [0.05, 0.05, 0.2]
    report["tracking_criterion"] = {
        "velocity_rmse_limits": tolerances,
        "units": ["m/s", "m/s", "rad/s"],
        "window": "entire upright portion including startup; full horizon survival required",
        "source": "local basic validation acceptance thresholds",
    }
    for case in report["cases"]:
        case["tracking_pass"] = case["survived"] and all(
            error <= limit for error, limit in zip(case["upright_velocity_rmse"], tolerances))
    report["tracking_passed"] = sum(c["tracking_pass"] for c in report["cases"])
    report["validation_gate"] = "pass" if report["tracking_passed"] == report["total"] else "fail"
    write_json(out / "report.json", report)
    lines = [
        f"# Microduck validation — {report['run']}", "",
        f"**Basic locomotion validation: {report['validation_gate'].upper()}.** "
        f"Survival: {report['survived']}/{report['total']}; "
        f"survival plus command tracking: {report['tracking_passed']}/{report['total']}.", "",
        "The exported ONNX was exercised through the pinned official Microduck "
        "inference code with BAM M6 XL330 actuators, projected gravity, the 61D "
        "observation layout and unfiltered 14D actions at 50 Hz. Physics ran at "
        "200 Hz on CPU. This is deterministic deployment inference; the original "
        "PPO learner ran on the Mac GPU.", "",
        "Each command has three 10-second trials: the exact upstream initial "
        "pose, then two small pose perturbations (up to 2° roll/pitch, 1 mm height, "
        "0.01 rad joints). Battery is fixed at 7.4 V with the default voltage sag. "
        "This does not test pushes, full domain randomization, uneven terrain, "
        "sensor noise, latency, or hardware.", "",
        "| Command | Requested vx / yaw | Mean achieved vx / yaw | Survival | Tracking |",
        "|---|---:|---:|---:|---:|",
    ]
    for name, command in COMMANDS.items():
        cases = [c for c in report["cases"] if c["case"] == name]
        mean = np.mean([c["upright_mean_velocity"] for c in cases], axis=0)
        lines.append(f"| {name} | {command[0]:.2f} m/s / {command[2]:+.2f} rad/s "
                     f"| {mean[0]:.4f} m/s / {mean[2]:+.4f} rad/s "
                     f"| {sum(c['survived'] for c in cases)}/{len(cases)} "
                     f"| {sum(c['tracking_pass'] for c in cases)}/{len(cases)} |")
    lines.extend([
        "", "A fall uses the training configuration's 70° trunk-tilt limit. "
        "MuJoCo warnings or non-finite state/action also fail survival. The local "
        "tracking limits are full-episode RMSE ≤0.05 m/s on vx/vy and ≤0.20 rad/s "
        "on yaw; these are screening thresholds, not hardware certification.", "",
        f"Policy SHA-256: `{report['policy_sha256']}`. "
        f"Upstream commit: `{report['commit']}`.", "",
        "[Full measurements](report.json). Videos and Nexus-compatible "
        "`*.rollout.json` files are saved beside this report.", "",
    ])
    (out / "report.md").write_text("\n".join(lines))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--seconds", type=float, default=10)
    parser.add_argument("--seeds", type=int, default=3)
    parser.add_argument("--render", action="store_true")
    args = parser.parse_args()
    if args.seconds <= 0 or not math.isfinite(args.seconds) or args.seeds < 1:
        parser.error("seconds must be finite and positive; seeds must be >= 1")
    repo, run = args.repo.resolve(), args.run_dir.resolve()
    commit = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
    if commit != COMMIT or subprocess.check_output(["git", "-C", str(repo), "status", "--porcelain", "--untracked-files=no"], text=True).strip():
        raise RuntimeError("Validation requires the clean pinned upstream source")
    verified = json.loads((run / "training/verified.json").read_text())
    policy_path = run / "policy.onnx"
    sha = hashlib.sha256(policy_path.read_bytes()).hexdigest()
    if verified["commit"] != COMMIT or verified["onnx_sha256"] != sha:
        raise RuntimeError("Export provenance does not match the policy being evaluated")
    spec = importlib.util.spec_from_file_location("microduck_infer", repo / "scripts/infer_policy.py")
    ip = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(ip)
    out = run / "validation"
    out.mkdir(exist_ok=True)
    report = {"run": run.name, "commit": commit, "policy_sha256": sha,
              "contract": "61 raw observations -> baked ONNX normalizer -> 14 unfiltered actions",
              "method": "official CPU MuJoCo BAM M6 deployment rehearsal",
              "physics_dt_s": 0.005, "control_dt_s": DT,
              "battery_v": 7.4, "voltage_sag_gain": 0.1,
              "fall_criterion": f"trunk tilt > {FALL_ANGLE} degrees (training limit)",
              "scope": "flat ground; nominal spawn plus small initial pose perturbations; no pushes, sensor noise or full domain randomization",
              "status": "running", "cases": []}
    for name, command in COMMANDS.items():
        for seed in range(args.seeds):
            result = rehearse(ip, policy_path, out, name, command, seed, args.seconds,
                              args.render and seed == 0)
            report["cases"].append(result)
            write_json(out / "report.json", report)
            print(json.dumps({"event": "evaluation", **result}), flush=True)
    summarize(report, out)
    print(json.dumps({k: report[k] for k in ("status", "survived", "total", "survival_gate")}), flush=True)


if __name__ == "__main__":
    main()
