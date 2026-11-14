"""Hardware controller contract checks with recorded/fake I/O, never physical hardware."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import threading
import time
from types import SimpleNamespace as NS
import unittest
import numpy as np

ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT/'tools/training'))
from backends import Unitree,Microduck,contract_sha
from g1_env import capture,G1Adapter,LEGS


class HardwareContracts(unittest.TestCase):
    def test_g1_measured_observations_pd_limits_and_target_deadline(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);capture(root,ROOT,53,12)
            robot=Unitree.__new__(Unitree);robot.env=G1Adapter(root,ROOT,42)
            robot.names=list(robot.env.names);robot.cfg=dict(mode_machine=7,torque_limits=[1.]*29)
            motors=[NS(q=float(robot.env.data.qpos[robot.env.model.joint(n).qposadr[0]]),dq=0.) for n in robot.names]
            robot.low=NS(mode_machine=7,motor_state=motors,imu_state=NS(quaternion=[1.,0.,0.,0.],gyroscope=[.1,.2,.3]))
            robot.received=time.monotonic();robot.ramp=(time.monotonic()-4,np.array([m.q for m in motors]));robot.net=lambda obs:np.ones(12)*.2
            robot.io_lock=threading.Lock();robot.active=False;robot.io_error=None;robot.last_torques=[0.]*29
            obs=robot.observation([.2,0.,0.]);self.assertEqual(obs.shape,(53,));np.testing.assert_allclose(obs[40:43],[0.,0.,-1.]);np.testing.assert_allclose(obs[45:48],[.1,.2,.3])
            packet=robot.step([0.,0.,0.]);self.assertEqual(len(packet['targets']),29)
            for i,n in enumerate(LEGS):self.assertAlmostEqual(packet['targets'][i],float(np.clip([-.1,0,0,.3,-.2,0][i%6]+.1,*robot.env.ranges[i])))
            class Writer:
                def __init__(self):self.messages=[]
                def Write(self,msg):self.messages.append(copy.deepcopy(msg));return True
            robot.pub=Writer();robot.crc=NS(Crc=lambda _:123)
            robot.message=NS(motor_cmd=[NS() for _ in range(35)])
            thread=threading.Thread(target=robot.io_loop,daemon=True);thread.start()
            deadline=time.monotonic()+1
            while not robot.io_error and time.monotonic()<deadline:time.sleep(.01)
            self.assertIsNotNone(robot.io_error)
            commands=[m for m in robot.pub.messages if getattr(m.motor_cmd[0],'mode',0)==1]
            self.assertGreater(len(commands),5)
            self.assertTrue(all(abs(m.tau)<=1. for command in commands for m in command.motor_cmd[:29]))
            self.assertTrue(all(m.mode==0 and m.tau==0 for m in robot.pub.messages[-1].motor_cmd))
            self.assertFalse(robot.active)
            robot.low.mode_machine=8;robot.received=time.monotonic()
            with self.assertRaisesRegex(ValueError,'hardware mode'):robot.sensor()
    def test_microduck_live_observation_mapping_excludes_mouth(self):
        robot=Microduck.__new__(Microduck);robot.indices=list(range(9))+list(range(10,15));robot.action=np.zeros(14);robot.last=None
        home=list(robot.HOME[:9])+[2.]+list(robot.HOME[9:]);state=dict(t=1.,joints=home,imu=dict(quat=[1.,0.,0.,0.],gyro=[.1,.2,.3]))
        robot.stream=NS(state=lambda:copy.deepcopy(state))
        obs=robot.observation([.2,.1,.3]);self.assertEqual(obs.shape,(61,));np.testing.assert_array_equal(obs[6:48],np.zeros(42));np.testing.assert_allclose(obs[48:51],[.2,.1,.3]);np.testing.assert_allclose(obs[3:6],[0.,0.,-1.])
        state['t']=1.02;state['joints'][10]+=.02
        changed=robot.observation([0.,0.,0.]);self.assertAlmostEqual(changed[20+9],1.)
    def test_microduck_sequential_swaps_retained_rollback_and_calibration(self):
        from backends import microduck_calibration,calibration_sha,check_microduck_runtime
        with tempfile.TemporaryDirectory() as directory:
            folder=Path(directory);original=folder/'original.onnx';original.write_bytes(b'original learned weights')
            runtime=folder/'robotd.toml';runtime.write_text('[policy]\naction_scale=1.0\nwalk="first.onnx"\n')
            first=calibration_sha(microduck_calibration(runtime));check_microduck_runtime(microduck_calibration(runtime))
            runtime.write_text('[policy]\naction_scale=1.0\nwalk="second.onnx"\nstand="none"\n')
            self.assertEqual(first,calibration_sha(microduck_calibration(runtime)))
            with self.assertRaisesRegex(ValueError,'action_scale'):check_microduck_runtime({'policy':{}})
            runtime.write_text('[policy]\naction_scale=0.9\n')
            self.assertNotEqual(first,calibration_sha(microduck_calibration(runtime)))
            class Daemon:
                def __init__(self):self.slots={'walk':str(original),'stand':None};self.pending=None;self.operations=[]
                def call(self,method,params=None):
                    self.operations.append((method,params))
                    if method=='robot.loadPolicy':
                        if self.pending:raise AssertionError('Overlapping policy swaps')
                        self.pending=params
                    elif method=='robot.policies':
                        if self.pending:
                            self.slots[self.pending['slot']]=self.pending['path'];self.pending=None
                        return dict(slots=[dict(slot=k,path=v) for k,v in self.slots.items()])
                    return dict(accepted=True)
            robot=Microduck.__new__(Microduck);robot.rpc=Daemon();robot.args=NS(state=folder);robot.backup=folder/'backup.json';robot.bundle=folder/'candidate'
            robot.arm();self.assertEqual(robot.rpc.slots['stand'],'none');self.assertEqual(robot.rpc.operations[-1],('robot.enable',{'on':True}))
            original.write_bytes(b'replaced by unrelated update')
            robot.rollback();saved=Path(robot.rpc.slots['walk']);self.assertEqual(saved.read_bytes(),b'original learned weights')
            self.assertEqual(robot.rpc.slots['stand'],'none')
            saved.write_bytes(b'corrupt')
            with self.assertRaisesRegex(ValueError,'snapshot changed'):robot.rollback()
    def test_open_duck_control_mapping_uses_captured_limits_and_fresh_imu(self):
        from open_duck import OpenDuck
        from duck_task_config import preset,JOINTS
        robot=OpenDuck.__new__(OpenDuck);config=preset('open_duck');robot.task=NS(config=config,default=np.zeros(14));robot.dt=.02
        robot.cfg=dict(joint_limits=[[-.1,.1]]*14);robot.names=JOINTS;robot.targets=np.zeros(14);robot.history=np.zeros((3,14));robot.phase=np.zeros(2);robot.count=0;robot.tilt_ticks=0
        robot.reference=NS(ep=NS(reference_frames=[None]*100));robot.initial=(time.monotonic()-4,np.zeros(14));robot.io_lock=threading.Lock()
        robot.observation=lambda _:np.zeros(101);robot.net=lambda _:np.ones(14)*100;robot.measured=(np.zeros(14),np.zeros(14),dict(accelero=np.array([0.,0.,9.81]),gyro=np.zeros(3)))
        result=robot.step([0.,0.,0.]);self.assertTrue(np.max(np.abs(result['targets']))<=.1);self.assertFalse(result['terminal'])
        robot.measured=(np.zeros(14),np.zeros(14),dict(accelero=np.array([9.81,0.,0.]),gyro=np.zeros(3)))
        for _ in range(5):result=robot.step([0.,0.,0.])
        self.assertTrue(result['terminal'])


if __name__=='__main__':unittest.main()
