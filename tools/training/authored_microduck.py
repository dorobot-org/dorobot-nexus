"""Apply captured editor values to the pinned official Microduck mjlab task.

PPO, rewards, observation terms, command generators, resets and BAM dynamics
remain upstream. Extensions supply per-servo calibration and optional limits.
"""
from contextlib import contextmanager
from copy import deepcopy
from dataclasses import dataclass, field, fields
import math
import re
from pathlib import Path
import mujoco

# Register the project before importing BAM, which otherwise triggers a
# circular plugin discovery import in the pinned dependency versions.
import mjlab_microduck.tasks  # noqa: F401
import torch
from mjlab.envs import ManagerBasedRlEnv
from mjlab.envs.mdp.actions import JointPositionAction, JointPositionActionCfg
from mjlab.rl import RslRlVecEnvWrapper
from mjlab.tasks.registry import load_env_cfg, load_rl_cfg
from mjlab_microduck.actuator.friction_dr_bam import FrictionDRBamActuator, FrictionDRBamActuatorCfg

from duck_task_config import JOINTS, configure_ppo, preset, validate
from microduck_worker import TASK


class CalibratedBam(FrictionDRBamActuator):
    def initialize(self, mj_model, model, data, device):
        super().initialize(mj_model,model,data,device)
        self.kp_scale=self.kp_scale*torch.tensor([self.cfg.calibration[n]['kp']/self.cfg.kp_fw for n in self._target_names],device=device)
        self.default_kp_scale=self.kp_scale.clone()

    def edit_spec(self,spec,target_names):
        super().edit_spec(spec,target_names)
        for actuator in self._mjs_actuators:
            name=actuator.target.name if hasattr(actuator.target,'name') else str(actuator.target)
            cap=self.cfg.calibration[name]['effort']
            if cap>0:
                actuator.forcerange=[max(-cap,actuator.forcerange[0]),min(cap,actuator.forcerange[1])]


@dataclass(kw_only=True)
class CalibratedBamCfg(FrictionDRBamActuatorCfg):
    calibration: dict=field(default_factory=dict)
    def build(self,entity,target_ids,target_names):
        return CalibratedBam(self,entity,target_ids,target_names)


class CalibratedAction(JointPositionAction):
    def __init__(self,cfg,env):
        super().__init__(cfg,env)
        self._offset+=torch.tensor([cfg.calibration[n]['offset'] for n in self._target_names],device=self.device)
        self._processed_actions=self._offset.clone()
        self.maximum=torch.tensor([cfg.calibration[n]['velocity'] or float('inf') for n in self._target_names],device=self.device)*env.step_dt

    def process_actions(self,actions):
        previous=self._processed_actions.clone()
        super().process_actions(actions)
        self._processed_actions=torch.clamp(self._processed_actions,min=previous-self.maximum,max=previous+self.maximum)

    def reset(self,env_ids=None):
        super().reset(env_ids)
        self._processed_actions[env_ids]=self._offset[env_ids]


@dataclass(kw_only=True)
class CalibratedActionCfg(JointPositionActionCfg):
    calibration: dict=field(default_factory=dict)
    def build(self,env):return CalibratedAction(self,env)


def apply_env(env_cfg,cfg):
    validate(cfg)
    base=preset('microduck')
    sim=cfg['simulation']
    env_cfg.scene.num_envs=cfg['ppo']['num_envs']
    env_cfg.sim.mujoco.timestep=sim['timestep'];env_cfg.decimation=sim['decimation'];env_cfg.episode_length_s=sim['episode_seconds']
    env_cfg.terminations['fell_over'].params['limit_angle']=math.radians(sim['max_tilt_degrees'])
    if sim['min_height']>0:
        from mjlab.managers import TerminationTermCfg
        env_cfg.terminations['nexus_height']=TerminationTermCfg(func=height_termination,params={'height':sim['min_height']})
    if sim['spawn_height']>0:
        env_cfg.events['reset_base'].params['pose_range']['z']=(sim['spawn_height'],sim['spawn_height'])
    for reward in cfg['rewards']:env_cfg.rewards[reward['name']].weight=reward['weight']
    terms=env_cfg.observations['actor'].terms
    env_cfg.observations['actor'].terms={o['name']:deepcopy(terms[o['name']]) for o in cfg['observations'] if o['enabled']}
    for o in cfg['observations']:
        if o['enabled']:env_cfg.observations['actor'].terms[o['name']].scale=o['scale']
    commands=cfg['commands'];ranges=env_cfg.commands['twist'].ranges
    ranges.lin_vel_x=tuple(commands['forward']);ranges.lin_vel_y=tuple(commands['lateral']);ranges.ang_vel_z=tuple(commands['yaw'])
    calibration={a['joint']:deepcopy(a) for a in cfg['actuators']}
    robot=env_cfg.scene.entities['robot']
    robot.articulation.actuators=tuple(CalibratedBamCfg(**{f.name:getattr(a,f.name) for f in fields(a) if f.init},calibration=calibration) for a in robot.articulation.actuators)
    action=env_cfg.actions['joint_pos']
    env_cfg.actions['joint_pos']=CalibratedActionCfg(**{f.name:getattr(action,f.name) for f in fields(action) if f.init},calibration=calibration)
    env_cfg.actions['joint_pos'].scale={re.escape(n):a['direction']*a['scale'] for n,a in calibration.items()}
    # Keep the upstream curriculum shape while honoring an edited initial
    # reward weight and learning horizon. A zero baseline uses an additive shift.
    weights={r['name']:r['weight'] for r in cfg['rewards']}
    initial={r['name']:r['weight'] for r in base['rewards']}
    mode=cfg.get('options',{}).get('curriculum','upstream')
    if mode not in ('upstream','hold','off'):raise ValueError('Unknown Microduck curriculum mode')
    for term in env_cfg.curriculum.values():
        name=term.params.get('reward_name')
        for key,stages in list(term.params.items()):
            if key.endswith('_stages') and isinstance(stages,list):
                for stage in stages:
                    if 'step' in stage:stage['step']=round(stage['step']*cfg['ppo']['steps_per_env']/24)
                    if name in weights and 'weight' in stage:
                        stage['weight']=stage['weight']*weights[name]/initial[name] if initial[name] else stage['weight']+weights[name]
                if mode=='hold':term.params[key]=stages[:1]
    if mode=='off':env_cfg.curriculum={}
    return env_cfg


def height_termination(env,height):
    return env.scene['robot'].data.root_link_pos_w[:,2]-env.scene.env_origins[:,2]<height


def capture_source(path):
    spec=load_env_cfg(TASK).scene.entities['robot'].spec_fn()
    for category,items,directory in [('mesh',spec.meshes,spec.meshdir),('texture',spec.textures,spec.texturedir)]:
        for i,item in enumerate(items):
            if not item.file:continue
            source=Path(spec.modelfiledir)/directory/item.file
            if not source.is_file():raise FileNotFoundError(source)
            name=f'{category}_{i}{source.suffix}'
            if not item.name:item.name=source.stem
            spec.assets[name]=source.read_bytes();item.file=name
    spec.meshdir='';spec.texturedir=''
    spec.compile()
    with Path(path).open('wb') as out:spec.to_zip(out)


def configs(cfg,play=False,snapshot=None):
    env=apply_env(load_env_cfg(TASK,play=play),cfg)
    if snapshot:
        def source():
            with Path(snapshot).open('rb') as model:return mujoco.MjSpec.from_zip(model)
        env.scene.entities['robot'].spec_fn=source
    agent=load_rl_cfg(TASK);configure_ppo(agent,cfg)
    agent.upload_model=False;agent.logger='tensorboard'
    return env,agent


class MappedEnv(RslRlVecEnvWrapper):
    def __init__(self,env,cfg,clip_actions=None):
        self.permutation=[a['action_index'] for a in cfg['actuators']]
        super().__init__(env,clip_actions)
    def step(self,actions):
        # Upstream's action-history and leg/head reward terms retain their
        # canonical physical order while the policy controls editable slots.
        return super().step(actions[...,self.permutation])


def build_env(cfg,play=False,num_envs=None,seed=42,snapshot=None,evaluation=False):
    env_cfg,agent=configs(cfg,play,snapshot)
    env_cfg.seed=seed
    if num_envs is not None:env_cfg.scene.num_envs=num_envs
    if evaluation:
        env_cfg.terminations={};env_cfg.curriculum={}
        env_cfg.events={n:e for n,e in env_cfg.events.items() if e.mode!='interval'}
        for group in env_cfg.observations.values():group.enable_corruption=False
        for command in env_cfg.commands.values():command.resampling_time_range=(1e9,1e9)
    from parallel_ppo import device as learner_device, world
    device=(learner_device('cuda') if world()>1 else 'cuda:0') if cfg['ppo']['device']=='cuda' else 'cpu'
    env=ManagerBasedRlEnv(cfg=env_cfg,device=device)
    factor=cfg['simulation']['friction']
    env.sim.model.geom_friction[...,0]*=factor
    env.sim.mj_model.geom_friction[:,0]*=factor
    return MappedEnv(env,cfg,agent.clip_actions),agent


@contextmanager
def export_configuration(cfg,snapshot=None):
    """Use the official exporter with the captured actor/env definitions."""
    import mjlab_microduck.export as exporter
    old_env,old_rl,old_class=exporter.load_env_cfg,exporter.load_rl_cfg,exporter.ManagerBasedRlEnv
    exporter.load_env_cfg=lambda task,play=False:configs(cfg,play,snapshot)[0] if task==TASK else old_env(task,play=play)
    exporter.load_rl_cfg=lambda task:configs(cfg,True)[1] if task==TASK else old_rl(task)
    try:yield
    finally:exporter.load_env_cfg,exporter.load_rl_cfg,exporter.ManagerBasedRlEnv=old_env,old_rl,old_class
