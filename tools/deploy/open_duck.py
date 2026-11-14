"""Open Duck Mini v2 adapter for the official Feetech HWI and BNO055 runtime."""
import json
from pathlib import Path
import queue
import threading
import time
import numpy as np
from backends import Hardware


class OpenDuck(Hardware):
    def connect(self):
        from mini_bdx_runtime.duck_config import DuckConfig
        from mini_bdx_runtime.rustypot_position_hwi import HWI
        from mini_bdx_runtime.raw_imu import Imu
        from mini_bdx_runtime.feet_contacts import FeetContacts
        from duck_task_config import JOINTS
        config=DuckConfig(config_json_path=self.cfg['duck_config'])
        self.hwi=HWI(config,usb_port=self.cfg.get('serial_port','/dev/ttyACM0'))
        self.imu=Imu(sampling_freq=50,upside_down=config.imu_upside_down)
        self.contacts=FeetContacts()
        self.names=list(self.hwi.joints)
        if self.names!=JOINTS:raise ValueError('Open Duck runtime motor order is incompatible')
        if config.joints_offset!=self.cfg['joint_offsets']:raise ValueError('Open Duck motor offsets differ from target commissioning')
        self.sample_queue=queue.Queue(maxsize=1);self.io_lock=threading.Lock()
        self.active=False;self.goal=None;self.sent=None;self.io_error=None;self.updated=0.
        # The sole motor I/O thread enforces a local command deadline, independent of HTTP.
        threading.Thread(target=self.io_loop,daemon=True).start()
        self.sample()
        return dict(robot=self.args.robot,transport='Open Duck official HWI / BNO055',joints=self.names)
    def io_loop(self):
        while True:
            start=time.monotonic()
            try:
                with self.io_lock:
                    if self.active and start-self.updated>.15:
                        self.hwi.turn_off();self.active=False;self.io_error='Actuator command deadline expired'
                    if self.active and self.goal is not None:
                        self.hwi.set_position_all(dict(zip(self.names,self.goal)))
                        self.sent=self.goal.copy()
                    q=self.hwi.get_present_positions();dq=self.hwi.get_present_velocities()
                if q is None or dq is None:raise ValueError('Open Duck motor read failed')
                sample=(time.monotonic(),np.asarray(q),np.asarray(dq))
                try:self.sample_queue.get_nowait()
                except queue.Empty:pass
                self.sample_queue.put_nowait(sample)
            except Exception as error:
                self.io_error=str(error)
                try:
                    with self.io_lock:
                        if self.active:self.hwi.turn_off();self.active=False
                except Exception:pass
            time.sleep(max(.002,self.dt-(time.monotonic()-start)))
    def sample(self):
        if self.io_error:raise RuntimeError(self.io_error)
        timestamp,q,dq=self.sample_queue.get(timeout=.2)
        if time.monotonic()-timestamp>.15 or not np.isfinite(q).all() or not np.isfinite(dq).all():raise ValueError('Invalid/stale Open Duck sensors')
        # Consume a fresh queued IMU sample; get_data() can silently reuse stale values.
        imu=self.imu.imu_queue.get(timeout=.2)
        if not all(np.isfinite(imu[k]).all() for k in ('gyro','accelero')):raise ValueError('Invalid Open Duck IMU')
        return q,dq,imu
    def prepare(self,bundle):
        super().prepare(bundle)
        from interactive_env import MujocoAdapter
        self.reference=MujocoAdapter(bundle,self.args.root,42)
        if not self.reference.duck:raise ValueError('Open Duck hardware needs its joystick observation contract')
        self.task=self.reference.task;self.dt=self.task.dt
        if abs(self.dt-.02)>1e-8:raise ValueError('Open Duck runtime is commissioned for 50 Hz')
        if not np.allclose(list(self.hwi.init_pos.values()),self.task.default,atol=.02):raise ValueError('Runtime home pose differs from captured training model')
        self.reset()
    def reset(self):
        self.history=np.zeros((3,14));self.targets=self.task.default.copy();self.phase=np.zeros(2);self.count=0
        self.command=np.zeros(7);self.reference.reset([0.,0.,0.]);self.initial=None;self.tilt_ticks=0
    def observation(self,command):
        q,dq,imu=self.sample();self.measured=(q,dq,imu)
        if not np.array_equal(self.command[:3],command):
            self.command[:3]=command;self.reference.observe(command)
        values=dict(angular_velocity=np.asarray(imu['gyro']),accelerometer=np.asarray(imu['accelero']),command=self.command,
                    joint_position=q-self.task.default,joint_velocity=dq,action_history=self.history.flatten(),
                    targets=self.targets,contacts=np.asarray(self.contacts.get()),phase=self.phase)
        return np.concatenate([values[o['name']]*o['scale'] for o in self.task.config['observations'] if o['enabled']])
    def arm(self):
        self.reset();q,_,_=self.sample();self.initial=(time.monotonic(),q.copy())
        kps=self.cfg['firmware_kp'];kds=self.cfg['firmware_kd']
        if len(kps)!=14 or len(kds)!=14:raise ValueError('Provide all 14 commissioned servo gains')
        with self.io_lock:
            self.hwi.set_kps(kps);self.hwi.set_kds(kds)
            # Set current measured positions before enabling torque: ramp occurs in step().
            self.hwi.set_position_all(dict(zip(self.names,q)))
            self.hwi.io.enable_torque(list(self.hwi.joints.values()))
            self.goal=q.copy();self.updated=time.monotonic();self.active=True
    def step(self,command):
        obs=self.observation(command);action=self.net(obs);q,dq,imu=self.measured
        actuators=self.task.config['actuators']
        goals=self.task.default+np.asarray([a['offset']+a['direction']*a['scale']*action[a['action_index']] for a in actuators])
        maximum=np.asarray([a['velocity'] for a in actuators])*self.dt
        self.targets=np.clip(goals,self.targets-maximum,self.targets+maximum)
        limits=np.asarray(self.cfg['joint_limits'])
        if limits.shape!=(14,2) or not np.isfinite(limits).all() or np.any(limits[:,0]>=limits[:,1]):raise ValueError('Invalid commissioned servo limits')
        fraction=min(1.,(time.monotonic()-self.initial[0])/3.)
        output=np.clip(self.initial[1]*(1-fraction)+self.targets*fraction,limits[:,0],limits[:,1])
        with self.io_lock:self.goal=output.copy();self.updated=time.monotonic()
        self.count+=1
        angle=self.count/len(self.reference.ep.reference_frames)*2*np.pi
        self.phase[:]=np.cos(angle),np.sin(angle)
        self.history[1:]=self.history[:-1].copy();self.history[0]=action
        accel=np.asarray(imu['accelero']);norm=float(np.linalg.norm(accel))
        tilted=norm>2. and accel[2]/norm<np.cos(np.radians(65.))
        self.tilt_ticks=self.tilt_ticks+1 if tilted else 0
        terminal=self.tilt_ticks>=5 or np.linalg.norm(imu['gyro'])>15.
        return dict(source='Open Duck HWI',joint_names=self.names,joints=[q.tolist()],velocities=dq.tolist(),
                    observation=obs.tolist(),action=action.tolist(),targets=output.tolist(),terminal=bool(terminal),terminal_reason='IMU acceleration tilt / angular-rate limit' if terminal else '')
    def stop(self):
        # Torque-off is explicit; this is a bench/support stop, not a balancing manoeuvre.
        with self.io_lock:
            self.active=False;self.goal=None;self.hwi.turn_off()
