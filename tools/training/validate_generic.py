"""Held-out velocity trials for a captured generic MuJoCo task and ONNX policy."""
import hashlib
from pathlib import Path
import numpy as np
import onnxruntime as ort
from microduck_motion import atomic_json, publish_validation


def validate(task, policy, output, run_dir, seconds=10.):
    session = ort.InferenceSession(str(policy), providers=["CPUExecutionProvider"])
    directory = Path(output)/"validation"; directory.mkdir(parents=True)
    # Fixed evaluation inputs are separate from training's random command range.
    commands = [("stand", [0.,0.,0.]), ("forward", [.15,0.,0.]), ("lateral", [0.,.1,0.]),
                ("turn_left", [0.,0.,.5]), ("turn_right", [0.,0.,-.5])]
    cases = []
    sim = task.config["simulation"]
    for name, command in commands:
        for seed in [10001, 20003, 30011]:
            episode = task.episode(seed, training=False); episode.reset(command)
            joints, bases, velocities, tilts = [], [], [], []
            observations, actions, torques = [], [], []
            first_fall = None
            for frame in range(round(seconds/task.dt)+1):
                joint, base = episode.pose(); joints.append(joint); bases.append(base)
                angular, linear = episode.velocity()
                velocities.append([*linear[:2], angular[2]])
                tilt = float(np.degrees(np.arccos(np.clip(episode.data.xmat[task.root_id].reshape(3,3)[2,2], -1, 1))))
                tilts.append(tilt)
                if tilt > sim["max_tilt_degrees"] or base[2] < sim["min_height"]: first_fall = frame*task.dt
                if first_fall is not None or frame == round(seconds/task.dt): break
                action = session.run(None, {session.get_inputs()[0].name: episode.obs[None]})[0][0]
                observations.append(episode.obs.tolist()); actions.append(action.tolist())
                episode.step(action); torques.append(episode.data.actuator_force[task.aids].tolist())
            velocity = np.asarray(velocities)
            rmse = np.sqrt(np.mean((velocity-command)**2, axis=0))
            passed = first_fall is None and bool(np.all(rmse <= [.05,.05,.2]))
            rollout = directory/f"{name}_seed{seed}.json"
            atomic_json(rollout, dict(name=f"{task.config['robot']} {name} seed {seed}", dt=task.dt,
                joint_names=task.pose_names, joints=joints, base=bases, observations=observations, actions=actions,
                torques=torques, actuator_joints=task.names, sample_timing="N+1 states with N transitions; observations/actions at transition start, torque at transition end"))
            cases.append(dict(case=name, seed=seed, command=command, survived=first_fall is None, tracking_pass=passed,
                max_tilt_deg=max(tilts), first_fall_seconds=first_fall, recorded_seconds=(len(joints)-1)*task.dt,
                upright_mean_velocity=velocity.mean(axis=0).tolist(), velocity_rmse=rmse.tolist(), rollout=str(rollout.resolve())))
    report = dict(status="completed", robot=task.config["robot"], contract=task.contract,
        policy_sha256=hashlib.sha256(Path(policy).read_bytes()).hexdigest(), cases=cases,
        survived=sum(c["survived"] for c in cases), total=len(cases), tracking_passed=sum(c["tracking_pass"] for c in cases),
        duration_seconds=seconds, fixed_base=sim["fixed_base"], thresholds=dict(max_tilt_deg=sim["max_tilt_degrees"],
            min_root_height=sim["min_height"], linear_rmse=.05, yaw_rmse=.2))
    report["validation_gate"] = "PASS" if report["tracking_passed"] == len(cases) else "FAIL"
    atomic_json(directory/"report.json", report)
    lines = [f"# {task.config['robot']} captured task validation", "", f"Result: **{report['validation_gate']}**. "
             f"Survival {report['survived']}/{len(cases)}; tracking {report['tracking_passed']}/{len(cases)}.", "",
             f"Deterministic ONNX actor; {seconds:g}-second trials using the captured MuJoCo model and three held-out reset seeds. "
             "Each trial ends at its first fall. Fixed-base tasks cannot track locomotion commands. These trials do not establish hardware readiness.", "",
             "| Command | Seed | Survived | Tracking | Velocity RMSE x / y / yaw |", "|---|---:|---|---|---|"]
    lines += [f"| {c['case']} | {c['seed']} | {c['survived']} | {c['tracking_pass']} | "+" / ".join(f"{v:.3f}" for v in c["velocity_rmse"])+" |" for c in cases]
    (directory/"report.md").write_text("\n".join(lines)+"\n")
    publish_validation(run_dir, output, robot=task.config["robot"])
    return report
