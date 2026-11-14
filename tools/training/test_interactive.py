"""Real physics, learned policy and pipe-lifecycle acceptance of interactive simulation."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

import numpy as np
from test_generic_task import URDF, fixture

HERE = Path(__file__).resolve().parent


class InteractiveTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.temp.name)
        model = cls.root / 'robot.urdf'
        model.write_text(URDF)
        cfg = fixture(model)
        cfg['ppo']['device'] = 'cpu'
        cfg['simulation'].update(fixed_base=False, spawn_height=.8, episode_seconds=10., max_tilt_degrees=175.)
        (cls.root / 'task.json').write_text(json.dumps(cfg))
        result = subprocess.run([sys.executable, str(HERE / 'generic_worker.py'), '--task-config', str(cls.root / 'task.json'),
                                 '--run-dir', str(cls.root / 'run')], capture_output=True, text=True, timeout=120)
        if result.returncode:
            raise AssertionError(result.stdout[-2000:] + result.stderr[-2000:])

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def test_real_policy_commands_forces_restart_and_recording(self):
        import onnxruntime as ort
        from interactive_env import MujocoAdapter
        from interactive_worker import Session
        adapter = MujocoAdapter(self.root / 'run', HERE.parents[1], 42)
        net = ort.InferenceSession(str(self.root / 'run/policy.onnx'), providers=['CPUExecutionProvider'])
        policy = lambda observation: net.run(None, {net.get_inputs()[0].name: observation[None]})[0][0]
        out = self.root / 'direct'
        out.mkdir()
        session = Session(adapter, policy, out, dict(run_id='run', robot='Acceptance arm', robot_id='arm', session_id='direct', checkpoint_sha256='a'*64))
        try:
            initial = session.packet['base']
            session.control(dict(id=1, op='step'))
            baseline = session.packet
            session.control(dict(id=2, op='restart'))
            np.testing.assert_array_equal(session.packet['base'], initial)
            session.control(dict(id=3, op='step'))
            np.testing.assert_array_equal(session.packet['base'], baseline['base'])
            np.testing.assert_array_equal(session.packet['action'], baseline['action'])
            session.control(dict(id=4, op='restart'))
            session.control(dict(id=5, op='push', force=[10., 0., 0.], seconds=.025))
            self.assertEqual(session.force_steps, 2)
            session.control(dict(id=6, op='step'))
            self.assertGreater(session.packet['base'][0][0] - baseline['base'][0][0], .0001)
            np.testing.assert_array_equal(session.packet['force_world'], [10., 0., 0.])
            session.control(dict(id=7, op='command', value=[.4, .2, .3]))
            session.control(dict(id=8, op='step'))
            self.assertEqual(session.force_steps, 0)
            self.assertEqual(session.packet['command'], [.4, .2, .3])
            obs = dict(zip(adapter.obs_names, session.packet['observation']))
            self.assertEqual([round(obs[f'command[{i}]'], 6) for i in range(3)], [.4, .2, .3])
            self.assertFalse(np.array_equal(session.packet['action'], baseline['action']))
            session.control(dict(id=9, op='command', value=[float('nan'), 0., 0.]))
            self.assertIn('finite', session.error)
            np.testing.assert_array_equal(session.command, [.4, .2, .3])
            samples = [json.loads(line) for line in (out / 'simulation-motion.jsonl').read_text().splitlines()]
            self.assertEqual([s['sequence'] for s in samples], [1, 2, 3, 4])
            self.assertEqual([s['reset_count'] for s in samples], [0, 1, 2, 2])
            for sample in samples:
                self.assertEqual(sample['source'], 'simulation')
                self.assertEqual(len(sample['targets']), len(sample['torques']))
                self.assertEqual(sample['checkpoint_sha256'], 'a'*64)
        finally:
            session.close()

    def test_pipe_pause_resume_ack_eof_and_wrong_robot(self):
        out = self.root / 'worker'
        with (self.root / 'worker.log').open('w') as log:
            proc = subprocess.Popen([sys.executable, str(HERE / 'interactive_worker.py'), '--checkpoint', str(self.root / 'run/policy.pt'),
                '--output', str(out), '--robot', 'Acceptance arm', '--robot-id', 'arm', '--run-id', 'run'],
                stdin=subprocess.PIPE, stdout=log, stderr=log, text=True)
            def wait(predicate):
                end = time.monotonic()+60
                while time.monotonic() < end:
                    try:
                        value = json.loads((out / 'status.json').read_text())
                        self.assertNotEqual(value['state'], 'failed', value)
                        if predicate(value):
                            return value
                    except FileNotFoundError:
                        pass
                    self.assertIsNone(proc.poll(), (self.root / 'worker.log').read_text()[-2000:])
                    time.sleep(.02)
                self.fail('Worker acknowledgement timeout')
            def send(serial, op):
                proc.stdin.write(json.dumps(dict(id=serial, op=op))+'\n')
                proc.stdin.flush()
                return wait(lambda s: s.get('ack', 0) >= serial)
            try:
                wait(lambda s: s['state'] == 'paused')
                s = send(1, 'resume')
                wait(lambda s: s['sequence'] >= 3)
                paused = send(2, 'pause')['sequence']
                time.sleep(.15)
                self.assertEqual(json.loads((out/'status.json').read_text())['sequence'], paused)
                self.assertEqual(send(3, 'step')['sequence'], paused+1)
                send(4, 'resume')
                wait(lambda s: s['sequence'] >= paused+2)
                proc.stdin.close()
                proc.wait(timeout=10)
                self.assertEqual(proc.returncode, 0)
                state = json.loads((out/'status.json').read_text())
                self.assertEqual(state['state'], 'stopped')
                lines = (out/'simulation-motion.jsonl').read_text().splitlines()
                self.assertEqual(len(lines), state['sequence'])
                provenance = json.loads((out/'policy/provenance.json').read_text())
                self.assertEqual(json.loads(lines[-1])['checkpoint_sha256'], provenance['checkpoint_sha256'])
            finally:
                if proc.poll() is None:
                    proc.kill()
                    proc.wait()
        rejected = self.root / 'wrong'
        result = subprocess.run([sys.executable, str(HERE / 'interactive_worker.py'), '--checkpoint', str(self.root / 'run/policy.pt'),
                '--output', str(rejected), '--robot', 'Microduck', '--robot-id', 'micro', '--run-id', 'run'],
                input='', capture_output=True, text=True, timeout=60)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(json.loads((rejected/'status.json').read_text())['state'], 'failed')
        self.assertFalse((rejected/'simulation-motion.jsonl').exists())

    def test_microduck_commands_refresh_cached_observations_and_real_world_force(self):
        from authored_microduck import capture_source
        from duck_task_config import preset
        from interactive_env import MicroduckAdapter
        bundle = self.root / 'micro'
        bundle.mkdir()
        cfg = preset('microduck')
        cfg['ppo']['device'] = 'cpu'
        (bundle/'task.json').write_text(json.dumps(cfg))
        capture_source(bundle/'model-source.mjz')
        adapter = MicroduckAdapter(bundle, HERE.parents[1], 42)
        try:
            adapter.observe([0., 0., 0.])
            updated = adapter.observe([.1, .2, .3])
            fields = dict(zip(adapter.obs_names, updated))
            np.testing.assert_allclose([fields[f'command[{i}]'] for i in range(3)], [.1, .2, .3])
            adapter.reset([0., 0., 0.])
            baseline = adapter.step(np.zeros(14), [0., 0., 0.])
            adapter.reset([0., 0., 0.])
            again = adapter.step(np.zeros(14), [0., 0., 0.])
            np.testing.assert_allclose(baseline['base'], again['base'], atol=1e-8, rtol=0.)
            adapter.reset([0., 0., 0.])
            pushed = adapter.step(np.zeros(14), [2., 0., 0.])
            self.assertGreater(np.max(np.abs(np.array(pushed['base'])-baseline['base'])), 1e-6)
            self.assertEqual(len(pushed['torques']), 14)
        finally:
            adapter.close()


if __name__ == '__main__':
    unittest.main()
