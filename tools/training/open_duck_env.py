"""Native MuJoCo port of Open Duck Playground's flat-terrain joystick task.

Derived from apirrone/Open_Duck_Playground (Apache-2.0), joystick.py.
Copyright 2025 DeepMind Technologies Limited, Antoine Pirrone, Steve Nguyen.
The pinned model, polynomial motion and NumPy reward functions stay upstream.
This is a Nexus port, not the upstream JAX/Brax trainer. See README.md.
"""
from contextlib import redirect_stdout
from pathlib import Path
import subprocess
import sys
import time

import mujoco
import numpy as np

from microduck_motion import atomic_json

COMMIT = "b9be205ac64488c23504ca42e5ec790337adeec3"
ROBOT = "Open Duck Mini v2"
TASK = "Open Duck Mini v2 · flat terrain · MPS PPO"
DT = 0.02
OBS_SIZE = 101
LOCKED_JOINTS = {"left_antenna": 0.0, "right_antenna": 0.0}


def check_source(repo):
    actual = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
    dirty = subprocess.check_output(["git", "-C", str(repo), "status", "--porcelain", "--untracked-files=no"], text=True).strip()
    if actual != COMMIT or dirty:
        raise RuntimeError(f"Open Duck Playground must be clean at {COMMIT}; found {actual}")


def load_model(repo):
    check_source(repo)
    # MuJoCo's mesh compilation is expensive. Cache only under a pinned source
    # revision and exact MuJoCo version; upstream files are never modified.
    cache = repo.parent / f"open-duck-{COMMIT}-{mujoco.__version__}.mjb"
    if cache.is_file():
        return mujoco.MjModel.from_binary_path(str(cache))
    xmls = repo / "playground/open_duck_mini_v2/xmls"
    assets = {p.name: p.read_bytes() for p in xmls.rglob("*") if p.is_file()}
    model = mujoco.MjModel.from_xml_string((xmls / "scene_flat_terrain.xml").read_text(), assets)
    temp = cache.with_suffix(".mjb.tmp")
    mujoco.mj_saveModel(model, str(temp), None)
    temp.replace(cache)
    return model


class DuckTask:
    def __init__(self, repo, config=None, snapshot=None):
        from duck_task_config import preset, validate, obs_size, contract
        self.config = config or preset('open_duck')
        validate(self.config)
        self.dt = self.config['simulation']['timestep'] * self.config['simulation']['decimation']
        self.obs_size = obs_size(self.config)
        self.contract = contract(self.config)
        repo = Path(repo).resolve()
        check_source(repo)
        self.model = mujoco.MjModel.from_binary_path(str(snapshot)) if snapshot else load_model(repo)
        self.model.opt.timestep = self.config['simulation']['timestep']
        if self.model.nu != 14 or self.model.nq != 21:
            raise RuntimeError("Expected the 14-actuator Open Duck flat-terrain model")
        sys.path.insert(0, str(repo))
        from playground.common import rewards_numpy
        from playground.common.poly_reference_motion_numpy import PolyReferenceMotion
        from playground.open_duck_mini_v2.custom_rewards_numpy import reward_imitation
        self.rewards = rewards_numpy
        self.imitation = reward_imitation
        with redirect_stdout(sys.stderr):
            self.reference = PolyReferenceMotion(str(repo / "playground/open_duck_mini_v2/data/polynomial_coefficients.pkl"))
        self.names = [self.model.actuator(i).name for i in range(self.model.nu)]
        if self.names != [a['joint'] for a in self.config['actuators']]:
            raise ValueError('Open Duck action mapping differs from the physical servo order')
        self.qadr = np.array([self.model.joint(n).qposadr[0] for n in self.names])
        self.vadr = np.array([self.model.joint(n).dofadr[0] for n in self.names])
        self.default = self.model.keyframe("home").ctrl.copy()
        self.floor = self.model.geom("floor").id
        self.feet = [self.model.geom(n).id for n in ("left_foot_bottom_tpu", "right_foot_bottom_tpu")]
        self.joint_noise = np.array([0.03 if "hip" in n else 0.05 if "knee" in n else 0.08 if "ankle" in n else 0 for n in self.names])
        if not snapshot:
            sim = self.config['simulation']
            self.model.geom_friction[:, 0] *= sim['friction']
            if sim['spawn_height'] > 0:
                self.model.keyframe('home').qpos[2] = sim['spawn_height']
            for i, a in enumerate(self.config['actuators']):
                self.model.actuator_gainprm[i, 0] = a['kp']
                self.model.actuator_biasprm[i, 1:3] = [-a['kp'], -a['kd']]
                self.model.actuator_forcelimited[i] = True
                self.model.actuator_forcerange[i] = [-a['effort'], a['effort']]

    def episode(self, seed, training=True):
        return DuckEpisode(self, seed, training)


class DuckEpisode:
    def __init__(self, task, seed, training):
        self.task, self.training = task, training
        self.model = task.model
        self.data = mujoco.MjData(self.model)
        self.rng = np.random.default_rng(seed)
        self.reset()

    def reset(self, command=None):
        mujoco.mj_resetDataKeyframe(self.model, self.data, self.model.keyframe("home").id)
        # Exact upstream reset ranges. Validation uses a small seeded perturbation
        # at home and fixed commands instead of exploratory training resets.
        if self.training:
            self.data.qpos[:2] += self.rng.uniform(-0.05, 0.05, 2)
            yaw = self.rng.uniform(-3.14, 3.14)
            self.data.qpos[3:7] = [np.cos(yaw / 2), 0, 0, np.sin(yaw / 2)]
            self.data.qpos[self.task.qadr] *= self.rng.uniform(0.5, 1.5, 14)
            self.data.qvel[:6] = self.rng.uniform(-0.05, 0.05, 6)
            self.data.ctrl[:] = self.data.qpos[self.task.qadr]
        else:
            self.data.qpos[self.task.qadr] += self.rng.normal(0, 0.003, 14)
        self.count = 0
        self.command = self.sample_command() if command is None else np.r_[command, np.zeros(4)]
        self.history = np.zeros((3, 14))
        self.delay = np.zeros((3, 14))
        self.targets = self.task.default.copy()
        self.phase = np.zeros(2)
        self.push_interval = round(self.rng.uniform(5, 10) / self.task.dt)
        self.next_command = 501 * DT
        self.reference_frames = None
        self.set_reference()
        mujoco.mj_forward(self.model, self.data)
        self.obs = self.observation()
        return self.obs

    def sample_command(self):
        if self.rng.random() < 0.1:
            return np.zeros(7)
        bounds = self.task.config['commands']
        return self.rng.uniform([bounds[k][0] for k in ('forward','lateral','yaw')] + [-0.34, -0.78, -1.5, -0.5],
                               [bounds[k][1] for k in ('forward','lateral','yaw')] + [1.1, 0.78, 1.5, 0.5])

    def set_reference(self):
        # Evaluate the pinned polynomial once per command instead of per step.
        prm = self.task.reference
        self.reference_frames = np.asarray([prm.get_reference_motion(*self.command[:3], i)
                                             for i in range(prm.nb_steps_in_period)])

    def sensor(self, name):
        return self.data.sensor(name).data.copy()

    def contacts(self):
        pairs = {(min(int(c.geom1), int(c.geom2)), max(int(c.geom1), int(c.geom2)))
                 for c in self.data.contact if c.dist <= 0}
        return np.array([(min(f, self.task.floor), max(f, self.task.floor)) in pairs for f in self.task.feet])

    def observation(self):
        gyro, accel = self.sensor("gyro"), self.sensor("accelerometer")
        pos, vel = self.data.qpos[self.task.qadr].copy(), self.data.qvel[self.task.vadr].copy()
        if self.training:
            gyro += self.rng.uniform(-0.1, 0.1, 3)
            accel += self.rng.uniform(-0.05, 0.05, 3)
            pos += self.rng.uniform(-1, 1, 14) * self.task.joint_noise
            vel += self.rng.uniform(-2.5, 2.5, 14)
        # In the pinned joystick.py the accelerometer .at[].set return is
        # discarded. Preserve the training contract (no added 1.3 bias).
        values = dict(angular_velocity=gyro, accelerometer=accel, command=self.command,
                      joint_position=pos-self.task.default, joint_velocity=vel,
                      action_history=self.history.flatten(), targets=self.targets, contacts=self.contacts(), phase=self.phase)
        obs = np.concatenate([values[o['name']]*o['scale'] for o in self.task.config['observations'] if o['enabled']]).astype(np.float32)
        if obs.shape != (self.task.obs_size,) or not np.isfinite(obs).all():
            raise RuntimeError("Open Duck observations disagree with the captured task")
        return obs

    def pose(self):
        # The upstream RL model locks both decorative antennas at zero. All
        # fourteen servo angles and the floating root are physical MuJoCo state.
        root = self.data.qpos[:7]
        return (self.data.qpos[self.task.qadr].tolist() + list(LOCKED_JOINTS.values()),
                root[:3].tolist() + root[4:7].tolist() + [float(root[3])])

    def step(self, action):
        action = np.asarray(action, dtype=np.float64)
        if action.shape != (14,) or not np.isfinite(action).all():
            raise RuntimeError("Open Duck policy must return 14 finite actions")
        self.recorded_observation = self.obs.copy()
        self.recorded_command = self.command[:3].copy()
        self.recorded_action = action.copy()
        self.count += 1
        phase_step = self.count * self.task.dt / DT
        index = int(phase_step) % len(self.reference_frames)
        angle = phase_step / len(self.reference_frames) * 2 * np.pi
        self.phase[:] = np.cos(angle), np.sin(angle)
        self.delay[1:] = self.delay[:-1].copy()
        self.delay[0] = action
        delayed = self.delay[self.rng.integers(0, 3)] if self.training else action
        actuators = self.task.config['actuators']
        targets = self.task.default + np.asarray([a['offset'] + a['direction']*a['scale']*delayed[a['action_index']] for a in actuators])
        maximum = np.asarray([a['velocity'] for a in actuators]) * self.task.dt
        self.targets = np.clip(targets, self.targets - maximum, self.targets + maximum)
        if self.training and self.count % self.push_interval == 0:
            theta, magnitude = self.rng.uniform(0, 2 * np.pi), self.rng.uniform(0.1, 1.0)
            self.data.qvel[:2] += magnitude * np.array([np.cos(theta), np.sin(theta)])
        self.data.ctrl[:] = self.targets
        mujoco.mj_step(self.model, self.data, nstep=self.task.config['simulation']['decimation'])
        # Refresh derived sensor/body values at the integrated physical qpos.
        mujoco.mj_forward(self.model, self.data)
        if not np.isfinite(self.data.qpos).all() or not np.isfinite(self.data.qvel).all():
            raise RuntimeError("Nonfinite Open Duck physics state")
        local, gyro, contacts = self.sensor("local_linvel"), self.sensor("gyro"), self.contacts()
        r = self.task.rewards
        raw = dict(track_linear_velocity=r.reward_tracking_lin_vel(self.command, local, 0.01),
                   track_angular_velocity=r.reward_tracking_ang_vel(self.command, gyro, 0.01),
                   torque_l2=r.cost_torques(self.data.actuator_force), action_rate_l2=r.cost_action_rate(action, self.history[0]),
                   alive=1., stand_still=r.cost_stand_still(self.command, self.data.qpos[self.task.qadr],self.data.qvel[self.task.vadr],self.task.default),
                   imitation=self.task.imitation(self.data.qpos[:7],self.data.qvel[:6],self.data.qpos[self.task.qadr],self.data.qvel[self.task.vadr],contacts,self.reference_frames[index],self.command,True))
        self.reward_terms = {term['name']:float(raw[term['name']]*term['weight']*self.task.dt) for term in self.task.config['rewards']}
        reward = float(np.clip(sum(self.reward_terms.values()), 0, 10000))
        sim = self.task.config['simulation']
        fallen = bool(self.sensor("upvector")[-1] < np.cos(np.radians(sim['max_tilt_degrees'])) or self.data.qpos[2] < sim['min_height'])
        timeout = self.count*self.task.dt >= sim['episode_seconds'] and not fallen
        self.obs = self.observation()
        self.history[1:] = self.history[:-1].copy()
        self.history[0] = action
        if self.training and self.count*self.task.dt >= self.next_command:
            self.next_command += 501*DT
            self.command = self.sample_command()
            self.set_reference()
        return self.obs, reward, fallen, timeout


class LiveMotion:
    def __init__(self, run_dir, task):
        self.root, self.task = Path(run_dir), task
        self.names = task.names + list(LOCKED_JOINTS)
        self.seq = self.resets = 0
        self.last = 0.0
        self.packet = None
        self.log = (self.root / "training-motion.jsonl").open("w", buffering=1)

    def step(self, episode, reset):
        import json
        from duck_task_config import OPEN_OBS
        self.seq += 1
        joints, base = episode.pose()
        self.packet = dict(version=1, run_id=self.root.name, robot=ROBOT, source="training", phase="training",
                           sequence=self.seq, wall_time=time.time(), sim_time=self.seq * self.task.dt, env_index=0,
                           reset_count=self.resets, finished=False, command=episode.recorded_command.tolist(),
                           dt=self.task.dt, joint_names=self.names, joints=[joints], base=[base], locked_joints=LOCKED_JOINTS,
                           observation=episode.recorded_observation.tolist(), next_observation=episode.obs.tolist(),
                           action=episode.recorded_action.tolist(), targets=episode.targets.tolist(),
                           torques=episode.data.actuator_force.tolist(), actuator_joints=self.task.names,
                           observation_names=[f"{o['name']}[{i}]" for o in self.task.config['observations'] if o['enabled'] for i in range(OPEN_OBS[o['name']])],
                           reward_terms=episode.reward_terms, terminal=bool(reset),
                           sample_timing="observation and command before action; pose, next_observation and torque after control step, before reset")
        self.log.write(json.dumps(self.packet, allow_nan=False) + "\n")
        if time.monotonic() - self.last >= 0.05:
            self.last = time.monotonic()
            atomic_json(self.root / "live-motion.json", self.packet)
        self.resets += int(reset)

    def close(self):
        if self.packet:
            self.packet["finished"] = True
            atomic_json(self.root / "live-motion.json", self.packet)
        self.log.close()
