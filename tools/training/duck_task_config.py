"""Captured configuration shared by the specialized duck task adapters."""
import hashlib
import json
from pathlib import Path

JOINTS = ['left_hip_yaw', 'left_hip_roll', 'left_hip_pitch', 'left_knee', 'left_ankle',
          'neck_pitch', 'head_pitch', 'head_yaw', 'head_roll', 'right_hip_yaw',
          'right_hip_roll', 'right_hip_pitch', 'right_knee', 'right_ankle']
OPEN_OBS = dict(angular_velocity=3, accelerometer=3, command=7, joint_position=14,
                joint_velocity=14, action_history=42, targets=14, contacts=2, phase=2)
OPEN_REWARDS = ['track_linear_velocity', 'track_angular_velocity', 'torque_l2', 'action_rate_l2', 'alive', 'stand_still', 'imitation']
MICRO_OBS = dict(base_ang_vel=3, projected_gravity=3, joint_pos=14, joint_vel=14, actions=14,
                 command=3, head_command=4, body_command=6)


def preset(kind):
    return json.loads((Path(__file__).with_name('task-presets')/(kind+'.json')).read_text())


def obs_size(cfg):
    widths = OPEN_OBS if cfg['kind']=='open_duck' else MICRO_OBS
    return sum(widths[o['name']] for o in cfg['observations'] if o['enabled'])


def contract(cfg):
    # Administrative names/paths and learning budget do not change the task.
    value = {k: cfg[k] for k in ('kind','robot','simulation','commands','observations','rewards','actuators')}
    value['hidden'] = cfg['ppo']['hidden']
    value['options'] = cfg.get('options', {})
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def validate(cfg):
    from generic_env import validate_config
    if cfg.get('kind') not in ('open_duck', 'microduck'):
        raise ValueError('Unknown specialized duck task')
    baseline=preset(cfg['kind'])
    if cfg['robot']!=baseline['robot'] or cfg['simulation']['fixed_base']:
        raise ValueError('The specialized duck task requires its matching floating-base robot')
    if [a['joint'] for a in cfg['actuators']]!=JOINTS or any(a['action_index'] is None for a in cfg['actuators']):
        raise ValueError('The specialized duck task requires all 14 servo channels in the captured joint order')
    expected_obs=OPEN_OBS if cfg['kind']=='open_duck' else MICRO_OBS
    if set(o['name'] for o in cfg['observations']) != set(expected_obs):
        raise ValueError('The specialized observation terms must match the task; disable unused terms explicitly')
    expected_rewards={r['name'] for r in baseline['rewards']}
    if set(r['name'] for r in cfg['rewards']) != expected_rewards:
        raise ValueError('Reward terms must match the captured specialized task')
    # Reuse all common numeric validation, with the specialized term vocabulary.
    validate_config(cfg, kinds=('open_duck','microduck'), observations=expected_obs, rewards=expected_rewards)
    if cfg['kind']=='microduck' and any(a['kd']!=0 for a in cfg['actuators']):
        raise ValueError('Microduck BAM uses firmware proportional gain; a separate PD derivative gain is not part of this motor model')
    if cfg['kind']=='microduck' and cfg.get('options', {}).get('curriculum', 'upstream') not in ('upstream', 'hold', 'off'):
        raise ValueError('Microduck curriculum must be upstream, hold or off')
    if cfg['kind']=='open_duck' and cfg['ppo']['num_envs'] < 4:
        raise ValueError('Open Duck requires at least four environments')


def configure_ppo(agent, cfg):
    p=cfg['ppo']
    agent.num_steps_per_env=p['steps_per_env'];agent.max_iterations=p['iterations'];agent.save_interval=p['checkpoint_every']
    agent.actor.hidden_dims=list(p['hidden']);agent.critic.hidden_dims=list(p['hidden'])
    a=agent.algorithm
    a.learning_rate=p['learning_rate'];a.num_learning_epochs=p['epochs'];a.num_mini_batches=p['minibatches']
    a.gamma=p['gamma'];a.lam=p['gae_lambda'];a.entropy_coef=p['entropy'];a.clip_param=p['clip']
