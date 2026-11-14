"""Real synchronized learning, checkpoint recovery and rank-failure tests."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from test_generic_task import URDF,fixture

HERE=Path(__file__).resolve().parent

class ParallelPpo(unittest.TestCase):
    def run_job(self,config,root,extra=()):
        result=subprocess.run([sys.executable,str(HERE/'parallel_worker.py'),'--processes','2','--worker',str(HERE/'generic_worker.py'),'--','--task-config',str(config),'--run-dir',str(root),*extra],capture_output=True,text=True,timeout=120)
        self.assertEqual(result.returncode,0,result.stdout[-3000:]+result.stderr[-1500:])
        samples=[json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
        return json.loads((root/'parallel-result.json').read_text()),[s for s in samples if s['event']=='sample']

    def test_real_two_process_learning_and_optimizer_continuation(self):
        import torch
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);model=root/'robot.urdf';model.write_text(URDF);cfg=fixture(model);cfg['ppo']['device']='cpu';config=root/'task.json';config.write_text(json.dumps(cfg))
            first,samples=self.run_job(config,root/'fresh')
            self.assertEqual([s['step'] for s in samples],[64,128])
            self.assertEqual(first['processes'],2);self.assertEqual(first['backend'],'gloo')
            self.assertEqual(len({r['seed'] for r in first['ranks']}),2)
            self.assertEqual(len({r['actor_sha256'] for r in first['ranks']}),1)
            self.assertEqual(len({r['critic_sha256'] for r in first['ranks']}),1)
            self.assertEqual(len({r['optimizer_sha256'] for r in first['ranks']}),1)
            for rank in first['ranks']:
                self.assertNotEqual(rank['initial_actor_sha256'],rank['actor_sha256']);self.assertEqual(rank['observation_samples'],128);self.assertGreater(rank['optimizer_states'],0)
            saved=torch.load(root/'fresh/policy.pt',weights_only=True);self.assertEqual(saved['infos']['env_steps'],128)
            second,samples=self.run_job(config,root/'continued',['--iterations','1','--resume-checkpoint',str(root/'fresh/policy.pt')])
            self.assertEqual(samples[-1]['step'],64)
            self.assertEqual({r['completed_iterations'] for r in second['ranks']},{3})
            self.assertEqual({r['observation_samples'] for r in second['ranks']},{192})
            self.assertNotEqual(second['ranks'][0]['actor_sha256'],first['ranks'][0]['actor_sha256'])
            self.assertTrue((root/'continued/policy.onnx').is_file())

    def test_one_failed_rank_stops_and_reaps_other_learners(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);script=root/'rank.py'
            script.write_text("import os,time\nif os.environ['RANK']=='1':\n time.sleep(.2)\n raise RuntimeError('injected learner failure')\nwhile True:time.sleep(.05)\n")
            result=subprocess.run([sys.executable,str(HERE/'parallel_worker.py'),'--processes','2','--worker',str(script),'--','--run-dir',str(root/'run')],capture_output=True,text=True,timeout=10)
            self.assertNotEqual(result.returncode,0);self.assertIn('injected learner failure',result.stdout)
            children=json.loads((root/'run/.parallel/processes.json').read_text())['workers']
            for pid in children:
                with self.assertRaises(ProcessLookupError):os.kill(pid,0)

if __name__=='__main__':unittest.main()
