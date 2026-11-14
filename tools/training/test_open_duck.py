"""Physics/viewport contract checks against the pinned imported robot."""
import json
from pathlib import Path
import tempfile
import unittest
import copy
import xml.etree.ElementTree as ET

import mujoco
import numpy as np

from open_duck_env import DuckTask, DT, LOCKED_JOINTS, LiveMotion

ROOT = Path(__file__).resolve().parents[2]


class OpenDuckContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.task = DuckTask(ROOT / "training-stack/open_duck_playground")

    def test_named_pose_matches_import_and_integrated_physics(self):
        urdf = ET.parse(ROOT / "robots/robot1788993861239/robot.urdf")
        names = {j.attrib["name"] for j in urdf.findall("joint") if j.attrib["type"] != "fixed"}
        self.assertEqual(set(self.task.names) | set(LOCKED_JOINTS), names)
        episode = self.task.episode(17, training=False)
        before = episode.data.qpos.copy()
        episode.step(np.ones(14))
        self.assertGreater(np.max(np.abs(episode.data.qpos - before)), 1e-4)
        self.assertLessEqual(np.max(np.abs(episode.targets - self.task.default)), 5.24 * DT + 1e-12)
        with tempfile.TemporaryDirectory() as path:
            motion = LiveMotion(path, self.task)
            motion.step(episode, True)
            motion.close()
            packet = json.loads((Path(path) / "live-motion.json").read_text())
            qpos = episode.data.qpos
            np.testing.assert_allclose(packet["joints"][0][:14], qpos[self.task.qadr])
            np.testing.assert_allclose(packet["base"][0], np.r_[qpos[:3], qpos[4:7], qpos[3]])
            self.assertEqual(packet["reset_count"], 0)
            self.assertTrue(packet["terminal"])
            self.assertEqual(len(packet["observation"]), 101)
            self.assertTrue(packet["finished"])

    def test_observation_history_and_seeded_evaluation_are_reproducible(self):
        a, b = [self.task.episode(12, training=False) for _ in range(2)]
        np.testing.assert_array_equal(a.obs, b.obs)
        self.assertEqual(a.obs.shape, (101,))
        for i in range(8):
            action = np.full(14, i / 10)
            a.step(action)
            b.step(action)
            np.testing.assert_array_equal(a.obs, b.obs)
            if i:
                # The returned next observation contains the previous action,
                # as in the pinned upstream task's pre-history-update order.
                np.testing.assert_allclose(a.obs[41:55], np.full(14, (i-1)/10), atol=1e-7)

    def test_falls_are_distinct_from_episode_time_limits(self):
        episode = self.task.episode(0, training=False)
        episode.count = 999
        _, _, fallen, timeout = episode.step(np.zeros(14))
        self.assertFalse(fallen)
        self.assertTrue(timeout)
        episode.reset()
        episode.data.qpos[3:7] = [0, 1, 0, 0]
        mujoco.mj_forward(self.task.model, episode.data)
        _, _, fallen, timeout = episode.step(np.zeros(14))
        self.assertTrue(fallen)
        self.assertFalse(timeout)

    def test_authored_task_consumes_mapping_rewards_observations_and_timing(self):
        cfg=copy.deepcopy(self.task.config)
        cfg['simulation'].update(timestep=.001,decimation=10,friction=.5,episode_seconds=.1)
        cfg['actuators'][0].update(action_index=1,direction=-1.,kp=5.,kd=.2,effort=.2,velocity=.5)
        cfg['actuators'][1]['action_index']=0
        for o in cfg['observations']:
            if o['name'] in ('accelerometer','action_history'):o['enabled']=False
        for r in cfg['rewards']:r['weight']=10. if r['name']=='alive' else 0.
        task=DuckTask(ROOT/'training-stack/open_duck_playground',cfg)
        episode=task.episode(7,training=False)
        action=np.zeros(14);action[1]=1.
        obs,reward,_,_=episode.step(action)
        self.assertEqual(obs.shape,(56,))
        self.assertAlmostEqual(reward,.1)
        self.assertAlmostEqual(episode.data.time,.01)
        self.assertAlmostEqual(episode.targets[0]-task.default[0],-.005)
        self.assertLessEqual(abs(episode.data.actuator_force[0]),.200001)
        self.assertAlmostEqual(task.model.geom_friction[task.floor,0],self.task.model.geom_friction[self.task.floor,0]*.5)
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'model.mjb';mujoco.mj_saveModel(task.model,str(path),None)
            restored=DuckTask(ROOT/'training-stack/open_duck_playground',cfg,path)
            same=restored.episode(7,training=False);same.step(action)
            np.testing.assert_array_equal(same.data.qpos,episode.data.qpos)
            self.assertEqual(restored.contract,task.contract)


if __name__ == "__main__":
    unittest.main()
