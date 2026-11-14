"""Configurable MuJoCo velocity task for imported URDF robots.

The configured action permutation, PD actuators, observation layout, rewards
and timing are used directly by physics and learning. A compiled MJZ snapshot
keeps all mesh assets and the initial pose with each run.
"""
import hashlib
import json
from pathlib import Path

import mujoco
import numpy as np


OBSERVATIONS = {"angular_velocity", "gravity", "command", "joint_position", "joint_velocity", "previous_action", "linear_velocity"}
REWARDS = {"track_linear_velocity", "track_angular_velocity", "upright", "alive", "joint_pose", "torque_l2", "action_rate_l2"}


def validate_config(cfg, kinds=("generic_mujoco",), observations=OBSERVATIONS, rewards=REWARDS):
    if cfg["version"] != 1 or cfg["kind"] not in kinds:
        raise ValueError("Unsupported generic task schema/backend")
    if not cfg["robot"] or not cfg["name"]:
        raise ValueError("Task and robot identity are required")
    actuators = cfg["actuators"]
    names = [a["joint"] for a in actuators]
    slots = [a["action_index"] for a in actuators if a["action_index"] is not None]
    if not 1 <= len(names) <= 256 or len(set(names)) != len(names) or not slots or sorted(slots) != list(range(len(slots))):
        raise ValueError("Actuator names and contiguous action slots must be unique")
    for a in actuators:
        if a["direction"] not in (-1, 1) or any(not np.isfinite(a[k]) for k in ("scale", "offset", "kp", "kd", "effort", "velocity")):
            raise ValueError("Invalid actuator calibration")
        minimum_limit = min(a['effort'],a['velocity'])
        if a['scale'] <= 0 or minimum_limit < 0 or (cfg['kind']!='microduck' and minimum_limit==0) or min(a["kp"], a["kd"]) < 0:
            raise ValueError("Actuator scales and limits must be positive, gains nonnegative")
    obs = [o["name"] for o in cfg["observations"]]
    reward_names = [r["name"] for r in cfg["rewards"]]
    if len(set(obs)) != len(obs) or not set(obs) <= set(observations) or not any(o["enabled"] for o in cfg["observations"]):
        raise ValueError("Invalid observation layout")
    if len(set(reward_names)) != len(reward_names) or not reward_names or not set(reward_names) <= set(rewards):
        raise ValueError("Invalid reward terms")
    if any(not np.isfinite(o["scale"]) or o["scale"] <= 0 for o in cfg["observations"]) or any(not np.isfinite(r["weight"]) for r in cfg["rewards"]):
        raise ValueError("Nonfinite observation or reward parameters")
    sim, ppo = cfg["simulation"], cfg["ppo"]
    if not .0001 <= sim["timestep"] <= .02 or not 1 <= sim["decimation"] <= 100 or not .001 <= sim["timestep"]*sim["decimation"] <= .2:
        raise ValueError("Invalid physics or control period")
    if not .1 <= sim["episode_seconds"] <= 300 or not .01 <= sim["friction"] <= 5 or not 0 <= sim["spawn_height"] <= 10 or not 0 <= sim["min_height"] <= 5 or not 5 <= sim["max_tilt_degrees"] <= 175:
        raise ValueError("Invalid simulation limits")
    for bounds in cfg["commands"].values():
        if len(bounds) != 2 or not -10 <= bounds[0] <= bounds[1] <= 10:
            raise ValueError("Invalid velocity command range")
    if not 1 <= ppo["num_envs"] <= 65536 or not 1 <= ppo["iterations"] <= 1000000 or not 4 <= ppo["steps_per_env"] <= 4096:
        raise ValueError("Invalid learning budget")
    if not 1 <= ppo["epochs"] <= 100 or not 1 <= ppo["minibatches"] <= ppo["num_envs"]*ppo["steps_per_env"] or not 1e-7 <= ppo["learning_rate"] <= .1:
        raise ValueError("Invalid PPO update parameters")
    if not 0 <= ppo["gamma"] <= 1 or not 0 <= ppo["gae_lambda"] <= 1 or not 0 <= ppo["entropy"] <= 1 or not .001 <= ppo["clip"] <= 1:
        raise ValueError("Invalid PPO objective parameters")
    if not ppo["hidden"] or len(ppo["hidden"]) > 8 or any(not 4 <= n <= 4096 for n in ppo["hidden"]) or ppo["checkpoint_every"] < 1:
        raise ValueError("Invalid network or checkpoint settings")
    if ppo["device"] not in ("cpu", "mps", "cuda"):
        raise ValueError("Invalid learner device")


class Task:
    def __init__(self, config, snapshot=None):
        validate_config(config)
        self.config = json.loads(json.dumps(config, allow_nan=False))
        self.dt = config["simulation"]["timestep"] * config["simulation"]["decimation"]
        self.n_actions = sum(a["action_index"] is not None for a in config["actuators"])
        self.names = [a["joint"] for a in config["actuators"]]
        if snapshot:
            with Path(snapshot).open("rb") as source:
                self.spec = mujoco.MjSpec.from_zip(source)
        else:
            path = Path(config["model"]).resolve(strict=True)
            self.spec = mujoco.MjSpec.from_file(str(path))
            # URDF compiler.meshdir may duplicate a prefix already present in
            # mesh filenames (the G1 URDF does this). Resolve the actual files.
            meshdir = self.spec.compiler.meshdir
            for i, mesh in enumerate(self.spec.meshes):
                candidates = [path.parent / mesh.file, path.parent / meshdir / mesh.file]
                source = next((p.resolve() for p in candidates if p.is_file()), None)
                if source is None:
                    raise FileNotFoundError(f"Missing model mesh: {mesh.file}")
                asset = f"mesh_{i}{source.suffix}"
                self.spec.assets[asset] = source.read_bytes()
                mesh.file = asset
            self.spec.compiler.meshdir = ""
            self.spec.compiler.fusestatic = False
            self.spec.compiler.discardvisual = False
            roots = self.spec.worldbody.bodies
            if len(roots) != 1:
                raise ValueError("The task requires one robot root body")
            root = roots[0]
            free = [j for j in self.spec.joints if j.type == mujoco.mjtJoint.mjJNT_FREE]
            if config["simulation"]["fixed_base"]:
                for joint in free:
                    self.spec.delete(joint)
            elif not free:
                root.add_freejoint(name="nexus_root")
            if len(free) > 1:
                raise ValueError("A robot task supports one floating base")
            self.spec.option.timestep = config["simulation"]["timestep"]
            self.spec.option.integrator = mujoco.mjtIntegrator.mjINT_IMPLICITFAST
            self.spec.worldbody.add_geom(name="nexus_floor", type=mujoco.mjtGeom.mjGEOM_PLANE,
                                         size=[0, 0, .1], friction=[config["simulation"]["friction"], .005, .0001])
            for geom in self.spec.geoms:
                if geom.contype:
                    geom.friction[0] = config["simulation"]["friction"]
            for actuator in list(self.spec.actuators):
                self.spec.delete(actuator)
            for a in config["actuators"]:
                joint = self.spec.joint(a["joint"])
                if joint is None or joint.type not in (mujoco.mjtJoint.mjJNT_HINGE, mujoco.mjtJoint.mjJNT_SLIDE):
                    raise ValueError(f"Actuator is not bound to a scalar physical joint: {a['joint']}")
                self.spec.add_actuator(name="nexus_"+a["joint"], target=a["joint"], trntype=mujoco.mjtTrn.mjTRN_JOINT,
                    gaintype=mujoco.mjtGain.mjGAIN_FIXED, gainprm=[a["kp"]]+[0]*9,
                    biastype=mujoco.mjtBias.mjBIAS_AFFINE, biasprm=[0, -a["kp"], -a["kd"]]+[0]*7,
                    forcelimited=True, forcerange=[-a["effort"], a["effort"]])
        self.model = self.spec.compile()
        self.root_id = self.model.body(self.spec.worldbody.bodies[0].name).id
        self.qadr = np.array([self.model.joint(n).qposadr[0] for n in self.names])
        self.vadr = np.array([self.model.joint(n).dofadr[0] for n in self.names])
        self.aids = np.array([self.model.actuator("nexus_"+n).id for n in self.names])
        self.lower, self.upper = [], []
        for name in self.names:
            j = self.model.joint(name)
            self.lower.append(j.range[0] if j.limited else -np.inf)
            self.upper.append(j.range[1] if j.limited else np.inf)
        self.lower, self.upper = np.asarray(self.lower), np.asarray(self.upper)
        self.neutral = np.clip(self.model.qpos0[self.qadr], self.lower, self.upper)
        if not snapshot:
            initial = self.model.qpos0.copy()
            initial[self.qadr] = self.neutral
            data = mujoco.MjData(self.model); data.qpos[:] = initial
            mujoco.mj_forward(self.model, data)
            low = []
            for i in range(self.model.ngeom):
                if self.model.geom(i).name == "nexus_floor" or not self.model.geom_contype[i]:
                    continue
                center, size = self.model.geom_aabb[i, :3], self.model.geom_aabb[i, 3:]
                rotation = data.geom_xmat[i].reshape(3, 3)
                low.append(float((data.geom_xpos[i] + rotation @ center)[2] - np.abs(rotation[2]) @ size))
            z = config["simulation"]["spawn_height"]
            shift = z - data.xpos[self.root_id, 2] if z > 0 else .015 - min(low, default=0)
            free = np.flatnonzero(self.model.jnt_type == mujoco.mjtJoint.mjJNT_FREE)
            if len(free):
                initial[self.model.jnt_qposadr[free[0]] + 2] += shift
            else:
                self.spec.worldbody.bodies[0].pos[2] += shift
            self.spec.add_key(name="nexus_home", qpos=initial.tolist(), ctrl=self.neutral.tolist())
            self.model = self.spec.compile()
        self.home = self.model.keyframe("nexus_home").id
        # All scalar joint names are rendered, including joints held neutral.
        self.pose_names = [self.model.joint(i).name for i in range(self.model.njnt)
                           if self.model.jnt_type[i] in (mujoco.mjtJoint.mjJNT_HINGE, mujoco.mjtJoint.mjJNT_SLIDE)]
        self.pose_qadr = [self.model.joint(n).qposadr[0] for n in self.pose_names]
        self.obs_size = sum(self.width(o["name"]) for o in config["observations"] if o["enabled"])
        self.contract = hashlib.sha256(json.dumps(dict(robot=config["robot"], kind=config["kind"], observations=config["observations"],
            simulation=config["simulation"], rewards=config["rewards"], commands=config["commands"],
            actuators=config["actuators"], hidden=config["ppo"]["hidden"], joints=self.pose_names), sort_keys=True).encode()).hexdigest()

    def width(self, name):
        return len(self.names) if name in ("joint_position", "joint_velocity") else self.n_actions if name == "previous_action" else 3

    def save_snapshot(self, path):
        with Path(path).open("wb") as destination:
            self.spec.to_zip(destination)

    def episode(self, seed, training=True):
        return Episode(self, seed, training)


class Episode:
    def __init__(self, task, seed, training):
        self.task, self.training = task, training
        self.model = task.model
        self.data = mujoco.MjData(self.model)
        self.rng = np.random.default_rng(seed)
        self.reset()

    def reset(self, command=None):
        mujoco.mj_resetDataKeyframe(self.model, self.data, self.task.home)
        self.data.qpos[self.task.qadr] = np.clip(self.task.neutral + self.rng.normal(0, .01, len(self.task.names)), self.task.lower, self.task.upper)
        self.command = np.asarray(command, dtype=float) if command is not None else np.array([self.rng.uniform(*self.task.config["commands"][k]) for k in ("forward", "lateral", "yaw")])
        self.count, self.total_reward = 0, 0.0
        self.action = np.zeros(self.task.n_actions)
        self.targets = self.task.neutral.copy()
        self.data.ctrl[self.task.aids] = self.targets
        mujoco.mj_forward(self.model, self.data)
        self.obs = self.observation()
        self.reward_terms = {}
        return self.obs

    def velocity(self):
        value = np.zeros(6)
        mujoco.mj_objectVelocity(self.model, self.data, mujoco.mjtObj.mjOBJ_BODY, self.task.root_id, value, 1)
        return value[:3], value[3:]

    def observation(self):
        angular, linear = self.velocity()
        values = dict(angular_velocity=angular, linear_velocity=linear,
            gravity=self.data.xmat[self.task.root_id].reshape(3, 3).T @ [0, 0, -1], command=self.command,
            joint_position=self.data.qpos[self.task.qadr]-self.task.neutral,
            joint_velocity=self.data.qvel[self.task.vadr], previous_action=self.action)
        obs = np.concatenate([values[o["name"]]*o["scale"] for o in self.task.config["observations"] if o["enabled"]]).astype(np.float32)
        if obs.shape != (self.task.obs_size,) or not np.isfinite(obs).all():
            raise RuntimeError("Invalid generic task observations")
        return obs

    def pose(self):
        quat = self.data.xquat[self.task.root_id]
        return self.data.qpos[self.task.pose_qadr].tolist(), self.data.xpos[self.task.root_id].tolist()+quat[1:].tolist()+[float(quat[0])]

    def step(self, action):
        action = np.asarray(action, dtype=float)
        if action.shape != (self.task.n_actions,) or not np.isfinite(action).all():
            raise ValueError("Policy action shape or values disagree with the task")
        previous = self.action.copy()
        desired, velocity = [], []
        for i, a in enumerate(self.task.config["actuators"]):
            signal = action[a["action_index"]] if a["action_index"] is not None else 0.0
            desired.append(self.task.neutral[i] + a["offset"] + a["direction"]*a["scale"]*signal)
            velocity.append(a["velocity"])
        step = np.asarray(velocity) * self.task.dt
        self.targets = np.clip(np.clip(desired, self.targets-step, self.targets+step), self.task.lower, self.task.upper)
        self.data.ctrl[self.task.aids] = self.targets
        mujoco.mj_step(self.model, self.data, nstep=self.task.config["simulation"]["decimation"])
        mujoco.mj_forward(self.model, self.data)
        if not np.isfinite(self.data.qpos).all() or not np.isfinite(self.data.qvel).all():
            raise RuntimeError("Nonfinite simulator state")
        angular, linear = self.velocity()
        upright = float(self.data.xmat[self.task.root_id].reshape(3, 3)[2, 2])
        raw = dict(track_linear_velocity=float(np.exp(-np.sum((linear[:2]-self.command[:2])**2)/.25)),
                   track_angular_velocity=float(np.exp(-(angular[2]-self.command[2])**2/.25)),
                   upright=max(0.0, upright), alive=1.0,
                   joint_pose=float(np.sum((self.data.qpos[self.task.qadr]-self.task.neutral)**2)),
                   torque_l2=float(np.sum(self.data.actuator_force[self.task.aids]**2)),
                   action_rate_l2=float(np.sum((action-previous)**2)))
        self.reward_terms = {r["name"]: raw[r["name"]]*r["weight"]*self.task.dt for r in self.task.config["rewards"]}
        reward = sum(self.reward_terms.values())
        self.count += 1; self.total_reward += reward; self.action = action.copy()
        sim = self.task.config["simulation"]
        fallen = bool(upright < np.cos(np.radians(sim["max_tilt_degrees"])) or self.data.xpos[self.task.root_id, 2] < sim["min_height"])
        timeout = self.count * self.task.dt >= sim["episode_seconds"] and not fallen
        self.obs = self.observation()
        return self.obs, reward, fallen, timeout
