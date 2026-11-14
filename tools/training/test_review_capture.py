"""Recording hooks must capture real transitions without substituting reset poses."""
import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
import numpy as np
import torch
from unittest.mock import patch

class ReviewCapture(unittest.TestCase):
    def test_microduck_pre_reset_physics_and_cached_policy_input(self):
        from authored_microduck import build_env
        from duck_task_config import preset
        from microduck_motion import TrainingMotion
        cfg=preset('microduck');cfg['ppo']['num_envs']=4
        cfg['simulation'].update(episode_seconds=.1,timestep=.002,decimation=10)
        cfg['options']['curriculum']='hold'
        env,_=build_env(cfg,seed=17)
        with TemporaryDirectory() as directory:
            motion=TrainingMotion(env,Path(directory),'training');motion.attach(env)
            expected=[];reset=motion.old_reset
            def capture(ids):
                if bool((ids==0).any()):expected.append(motion.physical())
                return reset(ids)
            motion.old_reset=capture
            try:
                before=env.get_observations()['actor'][0].detach().cpu().tolist()
                for i in range(3):
                    env.unwrapped.episode_length_buf[:]=env.unwrapped.max_episode_length-1
                    action=torch.full((4,14),.15+i*.1)
                    # Instrumentation must not resample or recompute policy inputs.
                    with patch.object(env,'get_observations',side_effect=AssertionError('resampled observation')):
                        obs,reward,done,extras=env.step(action)
                    self.assertTrue(bool(done[0]))
                    packet=motion.packet
                    np.testing.assert_array_equal(packet['observation'],before)
                    np.testing.assert_allclose(packet['action'],action[0].numpy())
                    np.testing.assert_allclose(packet['joints'],expected[-1]['joints'])
                    np.testing.assert_allclose(packet['torques'],expected[-1]['torques'])
                    self.assertFalse(packet['pose_after_reset'])
                    self.assertTrue(packet['terminal'])
                    self.assertEqual(len(packet['observation_names']),61)
                    self.assertEqual(packet['reset_count'],i)
                    before=obs['actor'][0].detach().cpu().tolist()
                motion.close()
                frames=[json.loads(line) for line in (Path(directory)/'training-motion.jsonl').read_text().splitlines()]
                self.assertEqual([f['sequence'] for f in frames],[1,2,3])
                self.assertEqual(len(frames[-1]['torques']),14)
            finally:
                env.close()

    def test_open_duck_full_control_stream_and_policy_input_alignment(self):
        from open_duck_env import DuckTask,LiveMotion
        task=DuckTask(Path(__file__).resolve().parents[2]/'training-stack/open_duck_playground')
        episode=task.episode(19,training=False)
        with TemporaryDirectory() as directory:
            motion=LiveMotion(Path(directory),task)
            original=[]
            for i in range(6):
                before=episode.obs.copy();action=np.full(14,.1+i*.01)
                episode.step(action);motion.step(episode,i==2)
                original.append((before.copy(),episode.data.actuator_force.copy()))
            motion.close()
            frames=[json.loads(line) for line in (Path(directory)/'training-motion.jsonl').read_text().splitlines()]
            self.assertEqual([f['sequence'] for f in frames],list(range(1,7)))
            for f,(obs,torques) in zip(frames,original):
                np.testing.assert_array_equal(f['observation'],obs)
                np.testing.assert_array_equal(f['torques'],torques)
            self.assertEqual(frames[3]['reset_count'],1)

if __name__=='__main__':unittest.main()
