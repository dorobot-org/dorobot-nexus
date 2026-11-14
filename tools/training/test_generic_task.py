"""Physics-level acceptance of edited task values, using a two-joint URDF."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
import struct

import numpy as np
import mujoco
from generic_env import Task, validate_config

URDF = '''<robot name="test_arm">
<link name="base"><inertial><mass value="1"/><inertia ixx=".1" iyy=".1" izz=".1" ixy="0" ixz="0" iyz="0"/></inertial><collision><geometry><box size=".1 .1 .1"/></geometry></collision></link>
<link name="one"><inertial><mass value=".2"/><inertia ixx=".01" iyy=".01" izz=".01" ixy="0" ixz="0" iyz="0"/></inertial><collision><geometry><sphere radius=".02"/></geometry></collision></link>
<link name="two"><inertial><mass value=".2"/><inertia ixx=".01" iyy=".01" izz=".01" ixy="0" ixz="0" iyz="0"/></inertial><collision><geometry><sphere radius=".02"/></geometry></collision></link>
<joint name="j1" type="revolute"><parent link="base"/><child link="one"/><origin xyz="0 0 .2"/><axis xyz="0 0 1"/><limit lower="-1" upper="1" effort="2" velocity="5"/></joint>
<joint name="j2" type="revolute"><parent link="base"/><child link="two"/><origin xyz=".2 0 .2"/><axis xyz="0 0 1"/><limit lower="-1" upper="1" effort="2" velocity="5"/></joint></robot>'''


def fixture(path):
    return dict(version=1, revision=1, name="Acceptance two-joint task", kind="generic_mujoco", robot="Acceptance arm", model=str(path),
        ppo=dict(num_envs=4, iterations=2, steps_per_env=8, learning_rate=.0003, epochs=2, minibatches=2,
                 gamma=.99, gae_lambda=.95, entropy=.01, clip=.2, hidden=[32, 16], device="mps", checkpoint_every=1),
        simulation=dict(timestep=.002, decimation=10, episode_seconds=.2, spawn_height=0., fixed_base=True,
                        friction=.8, min_height=.01, max_tilt_degrees=65.),
        commands=dict(forward=[-.1,.1], lateral=[0.,0.], yaw=[-.2,.2]),
        observations=[dict(name=n, enabled=True, scale=1.) for n in ("gravity", "command", "joint_position", "joint_velocity", "previous_action")],
        rewards=[dict(name="alive", weight=2.), dict(name="action_rate_l2", weight=-.1)],
        actuators=[dict(joint=f"j{i+1}", action_index=i, direction=1., scale=.5, offset=0., kp=3., kd=.1, effort=2., velocity=5.) for i in range(2)])


class GenericTaskTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name)/"robot.urdf"
        self.path.write_text(URDF)
        self.cfg = fixture(self.path)

    def tearDown(self):
        self.temp.cleanup()

    def test_action_permutation_direction_gains_and_limits_affect_physics(self):
        cfg = copy.deepcopy(self.cfg)
        cfg["actuators"][0].update(action_index=1, direction=-1., kp=20., effort=.03, velocity=.5)
        cfg["actuators"][1].update(action_index=0, offset=.2)
        task = Task(cfg); episode = task.episode(10)
        episode.data.qpos[task.qadr] = 0.
        mujoco.mj_forward(task.model, episode.data)
        original = episode.data.qpos.copy()
        episode.step([0., 1.])
        self.assertAlmostEqual(episode.targets[0], -.01)
        self.assertAlmostEqual(episode.targets[1], .1)
        self.assertLessEqual(abs(episode.data.actuator_force[task.aids[0]]), .03000001)
        self.assertLess(episode.data.qpos[task.qadr[0]], original[task.qadr[0]])
        self.assertGreater(episode.data.qpos[task.qadr[1]], original[task.qadr[1]])
        low = Task(self.cfg).episode(10); low.step([0., 1.])
        self.assertGreater(abs(low.data.actuator_force[low.task.aids[1]]), .03)

    def test_reward_observation_and_timing_are_consumed(self):
        cfg = copy.deepcopy(self.cfg)
        cfg["observations"] = [dict(name="command", enabled=True, scale=2.), dict(name="joint_position", enabled=False, scale=1.)]
        cfg["simulation"].update(timestep=.001, decimation=5)
        task = Task(cfg); episode = task.episode(8)
        np.testing.assert_allclose(episode.reset([.1,.2,.3]), [.2,.4,.6])
        _, reward, _, _ = episode.step([1., 0.])
        self.assertAlmostEqual(episode.data.time, .005)
        self.assertAlmostEqual(reward, (2.-.1)*.005)
        self.assertEqual(task.obs_size, 3)
        cfg["rewards"][0]["weight"] = 4.
        other = Task(cfg).episode(8)
        self.assertAlmostEqual(other.step([1., 0.])[1]-reward, .01)

    def test_portable_snapshot_restores_same_physics_without_source_urdf(self):
        first = Task(self.cfg)
        snapshot = Path(self.temp.name)/"model.mjz"; first.save_snapshot(snapshot)
        self.path.unlink()
        second = Task(self.cfg, snapshot)
        self.assertEqual(first.contract, second.contract)
        a, b = first.episode(5), second.episode(5)
        for _ in range(20):
            a.step([.5,-.25]); b.step([.5,-.25])
        np.testing.assert_allclose(a.data.qpos, b.data.qpos, atol=1e-12)
        self.assertEqual(a.pose(), b.pose())

    def test_invalid_contracts_fail_before_simulation(self):
        for change in (lambda c: c["actuators"][1].update(action_index=0),
                       lambda c: c["simulation"].update(min_height=float("nan")),
                       lambda c: c["observations"][0].update(name="invented_sensor"),
                       lambda c: c["ppo"].update(minibatches=999999)):
            cfg = copy.deepcopy(self.cfg); change(cfg)
            with self.assertRaises(ValueError): validate_config(cfg)

    def test_mesh_assets_are_inside_snapshot_after_source_is_removed(self):
        points=[(0.,0.,0.),(.02,0.,0.),(0.,.02,0.),(0.,0.,.02)]
        mesh=Path(self.temp.name)/"part.stl"
        triangles=[(0,2,1),(0,1,3),(0,3,2),(1,2,3)]
        mesh.write_bytes(b"Nexus test mesh".ljust(80,b"\0")+struct.pack("<I",4)+b"".join(
            struct.pack("<12fH",0.,0.,0.,*(v for i in tri for v in points[i]),0) for tri in triangles))
        self.path.write_text(URDF.replace('<sphere radius=".02"/>','<mesh filename="part.stl"/>',1))
        task=Task(self.cfg);snapshot=Path(self.temp.name)/"meshed.mjz";task.save_snapshot(snapshot)
        mesh.unlink();self.path.unlink()
        restored=Task(self.cfg,snapshot)
        self.assertEqual(restored.model.nmesh,1)
        self.assertTrue(np.isfinite(restored.episode(8).step([.1,.2])[0]).all())


if __name__ == "__main__": unittest.main()
