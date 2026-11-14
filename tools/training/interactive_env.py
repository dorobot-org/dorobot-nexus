"""Single-environment policy adapters; no automatic episode resets or viewer dependency."""
import copy
import math
from pathlib import Path

import mujoco
import numpy as np


class MujocoAdapter:
    def __init__(self, bundle, root, seed):
        import json
        from generic_env import Task
        from open_duck_env import DuckTask, LOCKED_JOINTS
        self.cfg = json.loads((bundle / 'task.json').read_text()) if (bundle / 'task.json').exists() else None
        self.duck = self.cfg is None or self.cfg['kind'] == 'open_duck'
        self.task = (DuckTask(root / 'training-stack/open_duck_playground', self.cfg,
                             bundle / 'model.mjb' if self.cfg else None) if self.duck else
                     Task(self.cfg, bundle / 'model.mjz'))
        self.cfg = self.task.config
        self.seed, self.dt = seed, self.task.dt
        self.names = self.task.names + list(LOCKED_JOINTS) if self.duck else self.task.pose_names
        self.actuator_names = self.task.names
        self.obs_names = [f"{o['name']}[{i}]" for o in self.cfg['observations'] if o['enabled']
                          for i in range(self.width(o['name']))]
        self.root_id = (int(self.task.model.jnt_bodyid[np.where(self.task.model.jnt_type == 0)[0][0]])
                        if self.duck else self.task.root_id)
        self.reset([0., 0., 0.])

    def width(self, name):
        if not self.duck:
            return self.task.width(name)
        from duck_task_config import OPEN_OBS
        return OPEN_OBS[name]

    def reset(self, command):
        self.ep = self.task.episode(self.seed, training=False)
        # Re-seed after constructor initialization so restart repeats exactly.
        self.ep.rng = np.random.default_rng(self.seed)
        self.ep.reset(command)

    def observe(self, command):
        if not np.array_equal(self.ep.command[:3], command):
            self.ep.command[:3] = command
            if self.duck:
                self.ep.set_reference()
        self.ep.obs = self.ep.observation()
        return self.ep.obs.copy()

    def pose(self):
        joints, base = self.ep.pose()
        return dict(joints=[joints], base=[base])

    def velocity(self):
        if self.duck:
            return np.r_[self.ep.sensor('gyro'), self.ep.sensor('local_linvel')]
        return np.concatenate(self.ep.velocity())

    def step(self, action, force):
        self.ep.data.xfrc_applied[self.root_id, :3] = force
        _, reward, fallen, timeout = self.ep.step(action)
        ids = list(range(self.task.model.nu)) if self.duck else self.task.aids
        return dict(**self.pose(), next_observation=self.ep.obs.tolist(), targets=self.ep.targets.tolist(),
                    torques=self.ep.data.actuator_force[ids].tolist(), reward_terms=self.ep.reward_terms,
                    terminal=bool(fallen or timeout), terminal_reason='fall' if fallen else 'episode limit' if timeout else '')

    def close(self):
        pass


class MicroduckAdapter:
    def __init__(self, bundle, root, seed):
        import json
        import torch
        from microduck_worker import check_source, TASK
        check_source(root / 'training-stack/microduck_rl')
        import mjlab_microduck.tasks  # noqa: F401
        from duck_task_config import JOINTS
        self.cfg = json.loads((bundle / 'task.json').read_text()) if (bundle / 'task.json').exists() else None
        if self.cfg:
            from authored_microduck import build_env
            cfg = copy.deepcopy(self.cfg)
            cfg['ppo']['device'] = 'cpu'
            self.env, _ = build_env(cfg, play=True, num_envs=1, seed=seed,
                                    snapshot=bundle / 'model-source.mjz', evaluation=True)
        else:
            from mjlab.tasks.registry import load_env_cfg, load_rl_cfg
            from mjlab.envs import ManagerBasedRlEnv
            from mjlab.rl import RslRlVecEnvWrapper
            cfg = load_env_cfg(TASK, play=True)
            cfg.seed, cfg.scene.num_envs = seed, 1
            cfg.terminations, cfg.curriculum = {}, {}
            cfg.events = {n: e for n, e in cfg.events.items() if e.mode != 'interval'}
            for group in cfg.observations.values():
                group.enable_corruption = False
            for command in cfg.commands.values():
                command.resampling_time_range = (1e9, 1e9)
            self.env = RslRlVecEnvWrapper(ManagerBasedRlEnv(cfg=cfg, device='cpu'), load_rl_cfg(TASK).clip_actions)
        self.base = self.env.unwrapped
        self.robot = self.base.scene['robot']
        self.seed, self.dt = seed, self.base.step_dt
        self.names = self.actuator_names = list(JOINTS)
        self.ids = [self.robot.joint_names.index(n) for n in self.names]
        data = self.robot.data
        model = self.base.sim.mj_model
        ids = data.indexing.ctrl_ids.cpu().tolist()
        force_names = [mujoco.mj_id2name(model, mujoco.mjtObj.mjOBJ_JOINT, int(model.actuator_trnid[i, 0])).split('/')[-1] for i in ids]
        self.force_ids = [force_names.index(n) for n in self.names]
        manager = self.base.observation_manager
        self.obs_names = [f'{n}[{i}]' for n, shape in zip(manager.active_terms['actor'], manager.group_obs_term_dim['actor']) for i in range(math.prod(shape))]
        free = int(data.indexing.free_joint_q_adr[0])
        jid = int(np.where(model.jnt_qposadr == free)[0][0])
        global_body = int(model.jnt_bodyid[jid])
        self.root_body = data.indexing.body_ids.cpu().tolist().index(global_body)
        self.reset([0., 0., 0.])

    def reset(self, command):
        import torch
        self.env.seed(self.seed)
        self.env.reset()
        robot, base = self.robot, self.base
        root = robot.data.default_root_state.clone()
        root[:, :3] += base.scene.env_origins
        height = (self.cfg['simulation']['spawn_height'] if self.cfg else 0.) or .125
        root[:, 2] = base.scene.env_origins[:, 2] + height
        robot.write_root_state_to_sim(root)
        joints = robot.data.default_joint_pos.clone()
        joints[:, self.ids] += torch.tensor(np.random.default_rng(self.seed).uniform(-.003, .003, len(self.ids)), dtype=torch.float32)
        robot.write_joint_state_to_sim(joints, torch.zeros_like(joints))
        for actuator in robot.actuators:
            actuator.reset()
        self.force([0., 0., 0.])
        base.sim.forward()
        base.sim.sense()
        self.count = 0
        self.observe(command)

    def observe(self, command):
        import torch
        self.command = np.asarray(command).copy()
        self.base.command_manager.get_command('twist')[:] = torch.tensor(command, dtype=torch.float32)
        for name in ('head_pose', 'body_pose'):
            self.base.command_manager.get_command(name)[:] = 0.
        # mjlab caches observations from env.step/reset. An interactive command
        # changes those inputs between steps, without advancing sensor history.
        self.base.observation_manager._obs_buffer = None
        return self.env.get_observations()['actor'][0].detach().cpu().numpy().copy()

    def pose(self):
        data = self.robot.data
        root = data.data.qpos[0, data.indexing.free_joint_q_adr].detach().cpu().tolist()
        origin = self.base.scene.env_origins[0].detach().cpu().tolist()
        return dict(joints=[data.joint_pos[0, self.ids].detach().cpu().tolist()],
                    base=[[root[i] - origin[i] for i in range(3)] + root[4:7] + [root[3]]])

    def force(self, force):
        import torch
        forces = torch.tensor([[force]], dtype=torch.float32)
        self.robot.write_external_wrench_to_sim(forces, torch.zeros_like(forces), body_ids=[self.root_body])

    def velocity(self):
        return np.r_[self.robot.data.root_link_ang_vel_b[0].detach().cpu().numpy(),
                     self.robot.data.root_link_lin_vel_b[0].detach().cpu().numpy()]

    def step(self, action, force):
        import torch
        self.force(force)
        result = self.env.step(torch.tensor(action[None], dtype=torch.float32))
        self.count += 1
        pose = self.pose()
        x, y = pose['base'][0][3:5]
        sim = self.cfg['simulation'] if self.cfg else dict(max_tilt_degrees=65., min_height=.065, episode_seconds=20.)
        fallen = 1 - 2*(x*x+y*y) < np.cos(np.radians(sim['max_tilt_degrees'])) or pose['base'][0][2] < sim['min_height']
        timeout = self.count*self.dt >= sim['episode_seconds']
        scale = self.dt if self.base.cfg.scale_rewards_by_dt else 1.
        return dict(**pose, next_observation=self.observe(self.command).tolist(),
                    targets=self.robot.data.joint_pos_target[0, self.ids].detach().cpu().tolist(),
                    torques=self.robot.data.actuator_force[0, self.force_ids].detach().cpu().tolist(),
                    reward_terms={n: v[0]*scale for n, v in self.base.reward_manager.get_active_iterable_terms(0)},
                    terminal=bool(fallen or timeout), terminal_reason='fall' if fallen else 'episode limit' if timeout else '')

    def close(self):
        self.env.close()


def load(bundle, root, robot, seed):
    import json
    if (bundle/'g1.json').is_file():
        if robot != 'Unitree G1-29dof': raise ValueError('G1 controller cannot drive another robot')
        from g1_env import G1Adapter
        return G1Adapter(bundle, root, seed)
    task = json.loads((bundle / 'task.json').read_text()) if (bundle / 'task.json').exists() else {}
    if task.get('kind') == 'generic_mujoco' or robot == 'Open Duck Mini v2':
        return MujocoAdapter(bundle, root, seed)
    if robot == 'Microduck':
        return MicroduckAdapter(bundle, root, seed)
    raise ValueError(f'Interactive physics adapter is not yet implemented for {robot}')
