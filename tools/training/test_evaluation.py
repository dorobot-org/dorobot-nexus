"""Independently recompute metrics from real measured control transitions."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import numpy as np
from test_generic_task import URDF, fixture

HERE=Path(__file__).resolve().parent

class Evaluations(unittest.TestCase):
    def test_paired_metrics_and_reports_match_real_trajectories(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);model=root/'robot.urdf';model.write_text(URDF);cfg=fixture(model)
            cfg['ppo'].update(device='mps',checkpoint_every=1);cfg['simulation'].update(episode_seconds=1.,fixed_base=False,spawn_height=.6)
            task=root/'task.json';task.write_text(json.dumps(cfg))
            trained=subprocess.run([sys.executable,str(HERE/'generic_worker.py'),'--task-config',str(task),'--run-dir',str(root/'run')],capture_output=True,text=True,timeout=120)
            self.assertEqual(trained.returncode,0,trained.stdout[-1000:]+trained.stderr[-2000:])
            output=root/'evaluation'
            cmd=[sys.executable,str(HERE/'evaluate_policy.py'),'--checkpoint',str(root/'run/policy.pt'),'--compare',str(root/'run/training/model_0.pt'),
                 '--output',str(output),'--robot',cfg['robot'],'--robot-id','fixture','--run-id','run','--seconds','.2','--seeds','10001,20003']
            result=subprocess.run(cmd,capture_output=True,text=True,timeout=120)
            self.assertEqual(result.returncode,0,result.stdout[-1500:]+result.stderr[-1500:])
            report=json.loads((output/'report.json').read_text());self.assertEqual(report['comparison']['pairs'],10)
            for policy in range(2):
                actual=[]
                for case in report['results'][policy]:
                    rows=[json.loads(l) for l in (output/case['telemetry']).read_text().splitlines()]
                    velocity=np.array([r['velocity'] for r in rows]);rmse=np.sqrt(np.mean((velocity-case['command'])**2,axis=0))
                    np.testing.assert_allclose(rmse,case['velocity_rmse'],rtol=0.,atol=1e-12)
                    self.assertEqual(len(rows),case['frames']);self.assertEqual(rows[-1]['base'][0],json.loads((output/case['rollout']).read_text())['base'][-1])
                    actual.append(rmse)
                np.testing.assert_allclose(np.mean(actual,axis=0),report['summaries'][policy]['mean_velocity_rmse'])
            expected=np.array(report['summaries'][1]['mean_velocity_rmse'])-report['summaries'][0]['mean_velocity_rmse']
            np.testing.assert_allclose(report['comparison']['mean_rmse_delta'],expected,atol=1e-12)
            self.assertGreater(np.linalg.norm(expected),1e-9)
            self.assertEqual(len((output/'cases.csv').read_text().splitlines()),21)
            self.assertTrue((output/'report.md').is_file())
            self.assertEqual(json.loads((output/'status.json').read_text())['state'],'completed')
            early=root/'cancel-before-start';early.with_suffix('.cancel').write_text('cancel')
            cmd[cmd.index('--output')+1]=str(early)
            result=subprocess.run(cmd,capture_output=True,text=True,timeout=30)
            self.assertEqual(json.loads((early/'status.json').read_text())['state'],'cancelled')
            self.assertFalse((early/'report.json').exists())
            cancelled=root/'cancelled';cmd[cmd.index('--output')+1]=str(cancelled)
            process=subprocess.Popen(cmd,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            import time
            end=time.monotonic()+20
            while not cancelled.exists() and time.monotonic()<end:time.sleep(.02)
            (cancelled/'cancel').write_text('cancel')
            process.wait(timeout=30)
            self.assertEqual(json.loads((cancelled/'status.json').read_text())['state'],'cancelled')
            self.assertFalse((cancelled/'report.json').exists())

    def test_g1_observation_history_forces_and_captured_model_identity(self):
        from g1_env import capture,G1Adapter
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);capture(root,HERE.parents[1],265,12)
            env=G1Adapter(root,HERE.parents[1],42)
            first=env.observe([.2,0.,0.]);self.assertEqual(first.shape,(265,))
            np.testing.assert_array_equal(first[:53],first[-53:])
            self.assertAlmostEqual(first[12],.2)
            env.step(np.ones(12)*.1,[0.,0.,0.]);next_observation=env.observe([.3,0.,0.])
            np.testing.assert_array_equal(next_observation[-53:-41],np.zeros(12))
            env.step(np.zeros(12),[0.,0.,0.]);last=env.observe([0.,0.,0.])
            np.testing.assert_allclose(last[-53:-41],.1)
            env.reset([0.,0.,0.]);env.observe([0.,0.,0.]);baseline=env.step(np.zeros(12),[0.,0.,0.])
            env.reset([0.,0.,0.]);env.observe([0.,0.,0.]);pushed=env.step(np.zeros(12),[100.,0.,0.])
            self.assertGreater(np.linalg.norm(np.array(pushed['base'])-baseline['base']),1e-5)
            self.assertEqual(len(pushed['torques']),29)
            (root/'model.mjb').write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError,'digest'):G1Adapter(root,HERE.parents[1],42)

if __name__=='__main__':unittest.main()
