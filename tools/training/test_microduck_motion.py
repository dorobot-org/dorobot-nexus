"""Pose conventions and reset accounting at the simulator/viewer boundary."""
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace as NS
import unittest
from unittest.mock import patch

import torch
from microduck_motion import TrainingMotion, publish_validation


class MotionContractTest(unittest.TestCase):
    def test_physical_pose_origin_quaternion_passive_joints_and_throttled_reset(self):
        names = [f"servo_{i}" for i in range(14)]
        names.insert(4, "passive_wheel")
        qpos = torch.tensor([[11., -7., 0.2, 0.9238795, 0.38268343, 0., 0.]])
        joints = torch.arange(15).reshape(1, -1) * 0.01
        data = NS(data=NS(qpos=qpos), indexing=NS(free_joint_q_adr=list(range(7))), joint_pos=joints)
        class Scene(dict):
            env_origins = torch.tensor([[10., -8., 0.]])
        scene = Scene(robot=NS(joint_names=names, data=data))
        env = NS(scene=scene, step_dt=0.02,
                 command_manager=NS(get_command=lambda name: torch.tensor([[0.2, 0.1, -0.3]])))
        env.unwrapped = env
        with TemporaryDirectory() as directory:
            root = Path(directory)
            publisher = TrainingMotion(env, root, "train")
            with patch("microduck_motion.time.monotonic", side_effect=[1., 1.001, 1.06]):
                publisher.step(torch.tensor([0]))
                publisher.step(torch.tensor([1]))  # every step is logged even when viewport publication is throttled
                publisher.step(torch.tensor([0]))
            publisher.close()
            frames = [json.loads(s) for s in (root / "training-motion.jsonl").read_text().splitlines()]
            self.assertEqual([f["sequence"] for f in frames], [1, 2, 3])
            self.assertEqual(frames[-1]["reset_count"], 1)
            self.assertAlmostEqual(frames[-1]["sim_time"], 0.06)
            pose = frames[-1]["base"][0]
            torch.testing.assert_close(torch.tensor(pose), torch.tensor([1., 1., 0.2, 0.38268343, 0., 0., 0.9238795]))
            self.assertEqual(len(frames[-1]["joint_names"]), 14)
            self.assertNotIn("passive_wheel", frames[-1]["joint_names"])
            self.assertAlmostEqual(frames[-1]["joints"][0][4], 0.05)
            self.assertTrue(json.loads((root / "live-motion.json").read_text())["finished"])

    def test_incomplete_evaluation_cannot_replace_latest_manifest(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "evaluations/iteration_002751"
            (output / "validation").mkdir(parents=True)
            (output / "training").mkdir()
            evidence = {"checkpoint": str(root / "training/model_2750.pt"), "onnx_sha256": "verified-hash"}
            (output / "training/verified.json").write_text(json.dumps(evidence))
            report = {"status": "running", "policy_sha256": "verified-hash"}
            (output / "validation/report.json").write_text(json.dumps(report))
            latest = root / "validation-preview.json"
            latest.write_text("previous evidence")
            with self.assertRaises(RuntimeError):
                publish_validation(root, output)
            self.assertEqual(latest.read_text(), "previous evidence")


if __name__ == "__main__":
    unittest.main()
