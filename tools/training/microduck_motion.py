"""Publish real training poses and completed validation manifests for Nexus."""
import json
from pathlib import Path
import time


def atomic_json(path, value):
    temp = path.with_suffix(path.suffix + ".tmp")
    temp.write_text(json.dumps(value, allow_nan=False) + "\n")
    temp.replace(path)


class TrainingMotion:
    def __init__(self, env, run_dir, phase):
        self.env = env.unwrapped
        self.robot = self.env.scene["robot"]
        self.root = Path(run_dir)
        self.phase = phase
        self.names = list(self.robot.joint_names)
        self.ids = [i for i, n in enumerate(self.names) if not n.startswith("passive_")]
        self.names = [self.names[i] for i in self.ids]
        if len(self.names) != 14:
            raise RuntimeError("Microduck live motion requires 14 named servo joints")
        self.seq = self.resets = 0
        self.last = 0.0
        self.packet = None
        self.log = (self.root / "training-motion.jsonl").open("x", buffering=1)
        self.wrapper = None
        self.terminal_state = None
        self.in_step = False
        self.before = None

    def attach(self, wrapper):
        """Read-only hooks capture the policy input and physical state before auto-reset."""
        import math
        import mujoco
        from types import MethodType
        self.wrapper = wrapper
        self.old_step, self.old_reset = wrapper.step, self.env._reset_idx
        model = self.env.sim.mj_model
        ids = self.robot.data.indexing.ctrl_ids.cpu().tolist()
        force_names = [mujoco.mj_id2name(model, mujoco.mjtObj.mjOBJ_JOINT, int(model.actuator_trnid[i, 0])).split('/')[-1] for i in ids]
        self.force_ids = [force_names.index(name) for name in self.names]
        manager = self.env.observation_manager
        self.observation_names = [f"{name}[{i}]" for name, shape in zip(manager.active_terms['actor'], manager.group_obs_term_dim['actor']) for i in range(math.prod(shape))]
        def reset_hook(env, ids):
            if self.in_step and bool((ids == 0).any()):
                self.terminal_state = self.physical()
            return self.old_reset(ids)
        def step_hook(env, actions):
            # Read the cached tensor used by the learner; calling get_observations
            # here would resample observation noise and alter the experiment.
            self.before = self.env.observation_manager._obs_buffer['actor'][0].detach().cpu().tolist()
            self.command_before = self.env.command_manager.get_command('twist')[0].detach().cpu().tolist()
            self.action_before = actions[0].detach().cpu().tolist()
            self.terminal_state = None
            self.in_step = True
            try:
                result = self.old_step(actions)
                self.step(result[2], result[0]['actor'][0].detach().cpu().tolist())
                return result
            finally:
                self.in_step = False
        self.env._reset_idx = MethodType(reset_hook, self.env)
        wrapper.step = MethodType(step_hook, wrapper)

    def physical(self):
        data = self.robot.data
        root = data.data.qpos[0, data.indexing.free_joint_q_adr].detach().cpu().tolist()
        origin = self.env.scene.env_origins[0].detach().cpu().tolist()
        result = dict(joints=[data.joint_pos[0, self.ids].detach().cpu().tolist()],
            base=[[root[i]-origin[i] for i in range(3)] + root[4:7] + [root[3]]])
        if self.wrapper is not None:
            scale = self.env.step_dt if self.env.cfg.scale_rewards_by_dt else 1.
            result.update(targets=data.joint_pos_target[0, self.ids].detach().cpu().tolist(),
                torques=data.actuator_force[0, self.force_ids].detach().cpu().tolist(),
                actuator_joints=self.names,
                reward_terms={name:values[0]*scale for name, values in self.env.reward_manager.get_active_iterable_terms(0)})
        return result

    def step(self, dones, next_observation=None):
        self.seq += 1
        done = bool(dones[0].item() > 0)
        physical = self.terminal_state if self.terminal_state is not None else self.physical()
        self.packet = {
            "version": 1, "run_id": self.root.name, "robot": "Microduck",
            "source": "training", "phase": self.phase, "sequence": self.seq,
            "wall_time": time.time(), "sim_time": self.seq * self.env.step_dt,
            "env_index": 0, "reset_count": self.resets, "finished": False,
            "command": self.command_before if self.wrapper is not None else self.env.command_manager.get_command('twist')[0].detach().cpu().tolist(),
            "dt": self.env.step_dt, "joint_names": self.names, **physical,
            "terminal": done, "pose_after_reset": done and self.terminal_state is None,
        }
        if self.wrapper is not None:
            self.packet.update(observation=self.before, next_observation=next_observation,
                observation_names=self.observation_names, action=self.action_before,
                sample_timing="cached policy observation before action; physical pose/torque/targets before automatic reset; next_observation after automatic reset when terminal; torque is last physics substep")
        self.log.write(json.dumps(self.packet, allow_nan=False) + "\n")
        now = time.monotonic()
        if now-self.last >= .05:
            self.last = now
            atomic_json(self.root / "live-motion.json", self.packet)
        self.resets += int(done)

    def close(self):
        if self.wrapper is not None:
            self.wrapper.step = self.old_step
            self.env._reset_idx = self.old_reset
        if self.packet:
            self.packet["finished"] = True
            atomic_json(self.root / "live-motion.json", self.packet)
        self.log.close()


def publish_validation(run_dir, evaluation_dir, robot="Microduck"):
    root, output = Path(run_dir).resolve(), Path(evaluation_dir).resolve()
    report_path = output / "validation/report.json"
    report = json.loads(report_path.read_text())
    evidence = json.loads((output / "training/verified.json").read_text())
    if report["status"] != "completed" or report["policy_sha256"] != evidence["onnx_sha256"]:
        raise RuntimeError("Cannot publish incomplete or mismatched validation evidence")
    checkpoint = Path(evidence["checkpoint"])
    iteration = int(checkpoint.stem.removeprefix("model_")) + 1
    manifest = {
        "version": 1, "run_id": root.name, "robot": robot, "source": "validation",
        "iteration": iteration, "checkpoint": str(checkpoint),
        "policy_sha256": report["policy_sha256"], "result": report["validation_gate"],
        "survived": report["survived"], "total": report["total"],
        "tracking_passed": report["tracking_passed"],
        "report": str(output / "validation/report.md"), "cases": report["cases"],
    }
    # Keep every checkpoint selectable and atomically update the latest index.
    atomic_json(output / "nexus-validation.json", manifest)
    atomic_json(root / "validation-preview.json", manifest)
