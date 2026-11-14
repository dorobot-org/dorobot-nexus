"""Incremental nominal-flat G1 controller matching Zealot's MuJoCo harness.

Observation/history, explicit PD and passive joint dynamics are taken from
zealot/scripts/sim2sim_g1_mujoco.py. This adapter never auto-resets.
"""
import hashlib
import json
from pathlib import Path

import mujoco
import numpy as np

LEGS = [f'{side}_{part}_joint' for side in ('left', 'right') for part in
        ('hip_pitch', 'hip_roll', 'hip_yaw', 'knee', 'ankle_pitch', 'ankle_roll')]
DEFAULT = np.array([-.1, 0., 0., .3, -.2, 0.]*2)
HELD = [('waist_yaw', 300., 5., 88.), ('waist', 300., 5., 50.),
        ('shoulder_pitch', 90., 2., 25.), ('shoulder_roll', 60., 1., 25.),
        ('shoulder', 20., .4, 25.), ('elbow', 60., 1., 25.), ('wrist', 4., .2, 25.)]


def capture(bundle, root, observations, actions):
    if actions != 12:
        raise ValueError('Native G1 controller requires the 12-leg-action policy contract')
    candidates = [(f, observations//f) for f in (53, 48, 45) if observations % f == 0]
    if len(candidates) != 1:
        raise ValueError('Ambiguous/unsupported G1 observation history')
    frame, history = candidates[0]
    source = root/'mujoco-stack/playground/mujoco_playground/_src/locomotion/g1/xmls/scene_mjx_feetonly_flat_terrain.xml'
    model = mujoco.MjModel.from_xml_path(str(source))
    mujoco.mj_saveModel(model, str(bundle/'model.mjb'), None)
    cfg = dict(version=1, kind='zealot_g1', robot='Unitree G1-29dof', frame=frame, history=history,
               dt=.02, timestep=.005, decimation=4, action_scale=.5, ankle_kp=40., ankle_kd=2.,
               held_pose='home', gait_speed_cap=.5, fall_height=.45, max_tilt_degrees=70.,
               episode_seconds=20., mujoco_version=mujoco.__version__,
               model_sha256=hashlib.sha256((bundle/'model.mjb').read_bytes()).hexdigest(),
               controller='Zealot AGILE nominal-flat MuJoCo controller; home upper-body hold')
    (bundle/'g1.json').write_text(json.dumps(cfg, indent=2))


class G1Adapter:
    def __init__(self, bundle, root, seed):
        self.cfg = json.loads((bundle/'g1.json').read_text())
        c = self.cfg
        if c['mujoco_version'] != mujoco.__version__ or hashlib.sha256((bundle/'model.mjb').read_bytes()).hexdigest() != c['model_sha256']:
            raise ValueError('G1 captured model digest or MuJoCo version changed')
        if c['frame'] not in (45, 48, 53) or not 1 <= c['history'] <= 20:
            raise ValueError('Unsupported G1 observation contract')
        self.model = mujoco.MjModel.from_binary_path(str(bundle/'model.mjb'))
        m = self.model
        m.opt.timestep = c['timestep']
        m.opt.integrator = mujoco.mjtIntegrator.mjINT_IMPLICITFAST
        m.opt.iterations, m.opt.ls_iterations = 100, 50
        m.opt.disableflags |= mujoco.mjtDisableBit.mjDSBL_ACTUATION
        self.q = np.array([m.joint(n).qposadr[0] for n in LEGS])
        self.v = np.array([m.joint(n).dofadr[0] for n in LEGS])
        self.ranges = np.array([m.joint(n).range for n in LEGS])
        gains = [(200., 5., 139.) if 'knee' in n else (100., 2.5, 88.) if 'hip' in n else
                 (c['ankle_kp'], c['ankle_kd'], 50.) for n in LEGS]
        self.kp, self.kd, self.effort = np.array(gains).T
        m.dof_damping[self.v], m.dof_frictionloss[self.v], m.dof_armature[self.v] = .001, .1, .02
        self.home = m.key('home').id
        self.names = [m.joint(i).name for i in range(m.njnt) if m.jnt_type[i] == mujoco.mjtJoint.mjJNT_HINGE]
        self.pose_q = [m.joint(n).qposadr[0] for n in self.names]
        self.held = []
        for n in self.names:
            if n in LEGS:
                continue
            for frag, kp, kd, effort in HELD:
                if frag in n:
                    qa, va = m.joint(n).qposadr[0], m.joint(n).dofadr[0]
                    target = m.key_qpos[self.home, qa] if c['held_pose'] == 'home' else 0.
                    self.held.append((qa, va, kp, kd, effort, target))
                    break
        if len(self.names) != 29 or len(self.held) != 17:
            raise ValueError('G1 model must contain 12 policy and 17 held joints')
        self.actuator_names = list(self.names)
        self.actuator_v = [m.joint(n).dofadr[0] for n in self.names]
        self.root_id = m.body('pelvis').id
        self.seed, self.dt = seed, c['dt']
        fields = [f'lag2_action[{i}]' for i in range(12)] + ['command_x', 'command_y', 'command_yaw', 'command_aux']
        fields += [f'position/{n}' for n in LEGS] + [f'velocity/{n}' for n in LEGS] + ['gravity_x', 'gravity_y', 'gravity_z', 'phase_sin', 'phase_cos']
        if c['frame'] >= 48: fields += ['gyro_x', 'gyro_y', 'gyro_z']
        if c['frame'] >= 53: fields += ['step_distance', 'step_height', 'step_sin', 'step_cos', 'step_valid']
        self.obs_names = [f'history{h}/{n}' for h in range(c['history']) for n in fields]
        self.reset([0., 0., 0.])

    def reset(self, command):
        self.data = mujoco.MjData(self.model)
        mujoco.mj_resetDataKeyframe(self.model, self.data, self.home)
        self.data.qpos[self.q] = DEFAULT
        for qa, va, kp, kd, effort, target in self.held: self.data.qpos[qa] = target
        yaw = np.random.default_rng(self.seed).uniform(-.3, .3)
        self.data.qpos[3:7] = [np.cos(yaw/2), 0., 0., np.sin(yaw/2)]
        self.data.qvel[:] = 0.
        mujoco.mj_forward(self.model, self.data)
        self.count, self.phase = 0, 0.
        self.previous_q = self.data.qpos[self.q].copy()
        self.actions = [np.zeros(12), np.zeros(12)]
        self.history = None
        self.command = np.asarray(command).copy()

    def observe(self, command):
        self.command = np.asarray(command).copy()
        q, data, c = self.data.qpos[self.q], self.data, self.cfg
        speed = float(np.linalg.norm(command))
        phase = self.phase
        if speed >= .1:
            period = max(.4, .8 + (.55-.8)*max(0., (min(speed, c['gait_speed_cap'])-.1)/.4))
            phase = (max(0, self.count-1)*self.dt/period) % 1.
        self.decision_phase = phase
        frame = np.zeros(c['frame'])
        frame[:12] = self.actions[0] if self.count >= 2 else 0.
        frame[12:15] = command
        frame[16:28] = q-DEFAULT
        frame[28:40] = (q-self.previous_q)/self.dt if self.count else 0.
        frame[40:43] = data.xmat[self.root_id].reshape(3, 3).T @ [0., 0., -1.]
        frame[43:45] = [np.sin(2*np.pi*phase), np.cos(2*np.pi*phase)]
        if c['frame'] >= 48: frame[45:48] = data.qvel[3:6]
        # The five step-cue slots stay zero on this explicit flat-floor task.
        self.pending_frame = frame
        history = [frame]*c['history'] if self.history is None else self.history[1:]+[frame]
        return np.concatenate(history).astype(np.float32)

    def pose(self):
        root = self.data.qpos[:7]
        return dict(joints=[self.data.qpos[self.pose_q].tolist()], base=[root[:3].tolist()+root[4:7].tolist()+[float(root[3])]])

    def velocity(self):
        value = np.zeros(6)
        mujoco.mj_objectVelocity(self.model, self.data, mujoco.mjtObj.mjOBJ_BODY, self.root_id, value, 1)
        return value

    def step(self, action, force):
        if np.shape(action) != (12,): raise ValueError('G1 requires 12 actions')
        c, d, m = self.cfg, self.data, self.model
        before = d.qpos[self.q].copy()
        self.history = ([self.pending_frame.copy() for _ in range(c['history'])] if self.history is None else self.history[1:]+[self.pending_frame.copy()])
        self.phase = self.decision_phase
        targets = np.clip(DEFAULT+c['action_scale']*action, self.ranges[:, 0], self.ranges[:, 1])
        all_targets = dict(zip(LEGS, targets))
        d.xfrc_applied[self.root_id, :3] = force
        for _ in range(c['decimation']):
            d.qfrc_applied[:] = 0.
            d.qfrc_applied[self.v] = np.clip(self.kp*(targets-d.qpos[self.q])-self.kd*d.qvel[self.v], -self.effort, self.effort)
            for qa, va, kp, kd, effort, target in self.held:
                d.qfrc_applied[va] = np.clip(kp*(target-d.qpos[qa])-kd*d.qvel[va], -effort, effort)
                all_targets[m.joint(int(m.dof_jntid[va])).name] = target
            mujoco.mj_step(m, d)
        self.previous_q = before
        self.actions = [self.actions[1], np.asarray(action).copy()]
        self.count += 1
        mujoco.mj_forward(m, d)
        fallen = d.qpos[2] < c['fall_height'] or d.xmat[self.root_id].reshape(3, 3)[2, 2] < np.cos(np.radians(c['max_tilt_degrees']))
        timeout = self.count*self.dt >= c['episode_seconds']
        return dict(**self.pose(), next_observation=self.observe(self.command).tolist(),
                    targets=[float(all_targets[n]) for n in self.names], torques=d.qfrc_applied[self.actuator_v].tolist(), reward_terms={},
                    terminal=bool(fallen or timeout), terminal_reason='fall' if fallen else 'episode limit' if timeout else '')

    def close(self): pass
