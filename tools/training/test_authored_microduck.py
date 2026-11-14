"""Actual mjlab/BAM acceptance for the editable official Microduck task."""
import copy
from pathlib import Path
import tempfile
import unittest
import mujoco
import torch
from authored_microduck import build_env,capture_source,configs
from duck_task_config import preset,contract


class AuthoredMicroduck(unittest.TestCase):
    def test_edits_reach_original_bam_physics_observations_and_curricula(self):
        c=preset('microduck');c['ppo'].update(num_envs=4,hidden=[32,16],iterations=2,steps_per_env=8,learning_rate=.0007)
        c['observations'][-1]['enabled']=False;c['observations'][0]['scale']=.5
        c['actuators'][0].update(action_index=1,direction=-1.,kp=180.,effort=.08,velocity=.2)
        c['actuators'][1]['action_index']=0
        c['simulation'].update(timestep=.0025,decimation=4,episode_seconds=.1,friction=.5,spawn_height=.13,min_height=.02,max_tilt_degrees=55.)
        c['options']['curriculum']='hold'
        next(r for r in c['rewards'] if r['name']=='action_rate_l2')['weight']=-.3
        native,agent=configs(c)
        self.assertEqual(agent.algorithm.learning_rate,.0007)
        self.assertEqual(native.curriculum['action_rate_weight'].params['weight_stages'],[{'step':0,'weight':-.3}])
        with tempfile.TemporaryDirectory() as directory:
            snapshot=Path(directory)/'model-source.mjz';capture_source(snapshot)
            with snapshot.open('rb') as f:spec=mujoco.MjSpec.from_zip(f)
            self.assertGreater(len(spec.assets),30)
            self.assertTrue(all(not Path(m.file).is_absolute() for m in spec.meshes))
            baseline=copy.deepcopy(c);baseline['simulation']['friction']=1.
            original,_=build_env(baseline,snapshot=snapshot,seed=8)
            try:
                original_friction=original.unwrapped.sim.model.geom_friction.clone()
                original_host_friction=original.unwrapped.sim.mj_model.geom_friction.copy()
            finally:original.close()
            # Simulator reconstructs its robot from the asset bundle.
            env,_=build_env(c,snapshot=snapshot,seed=8)
            try:
                torch.testing.assert_close(env.unwrapped.sim.model.geom_friction[...,0],original_friction[...,0]*.5)
                import numpy as np
                np.testing.assert_allclose(env.unwrapped.sim.mj_model.geom_friction[:,0],original_host_friction[:,0]*.5)
                self.assertEqual(tuple(env.get_observations()['actor'].shape),(4,55))
                robot=env.unwrapped.scene['robot'];bam=robot.actuators[0]
                torch.testing.assert_close(bam.kp_scale[:,0],torch.full((4,),.9))
                self.assertAlmostEqual(env.unwrapped.sim.mj_model.actuator_forcerange[0,1],.08)
                action=env.unwrapped.action_manager.get_term('joint_pos');neutral=action._offset.clone()
                command=torch.zeros(4,14);command[:,1]=1.
                observation,reward,done,extras=env.step(command)
                torch.testing.assert_close(action._processed_actions[:,0]-neutral[:,0],torch.full((4,),-.002),atol=1e-6,rtol=1e-6)
                self.assertAlmostEqual(env.unwrapped.step_dt,.01)
                self.assertTrue(torch.isfinite(reward).all())
                self.assertTrue(torch.isfinite(observation['actor']).all())
                self.assertEqual(env.unwrapped.cfg.rewards['action_rate_l2'].weight,-.3)
            finally:env.close()

    def test_contract_binds_physics_and_observations_but_not_output_path_or_budget(self):
        a=preset('microduck');b=copy.deepcopy(a);b['model']='/relocated/robot.urdf';b['ppo']['iterations']=1
        self.assertEqual(contract(a),contract(b))
        b['actuators'][0]['kp']=180.;self.assertNotEqual(contract(a),contract(b))
