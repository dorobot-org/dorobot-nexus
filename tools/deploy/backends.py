"""Target adapters. Hardware drivers import only when explicitly configured on the target."""
from __future__ import annotations
import hashlib
import json
from pathlib import Path
import socket
import shutil
import tomllib
import threading
import time

import numpy as np


def contract_description(bundle):
    source=json.loads((bundle/'provenance.json').read_text())
    config={k:source[k] for k in ('robot','observations','actions')}
    for name in ('task.json','g1.json'):
        if (bundle/name).is_file():
            task=json.loads((bundle/name).read_text())
            config[name]={k:v for k,v in task.items() if k not in ('ppo','rewards','model','name','revision')}
    return config


def contract_sha(bundle):return hashlib.sha256(json.dumps(contract_description(bundle),sort_keys=True).encode()).hexdigest()


def microduck_calibration(path):
    config=tomllib.loads(Path(path).read_text())
    # robot.loadPolicy persists slot filenames. They are deployment state, not calibration.
    policy=config.setdefault('policy',{})
    for key in ('walk','stand','sitstand','ground_pick','kick_left','kick_right','roulade'):
        policy.pop(key,None)
    return config


def calibration_sha(config):
    return hashlib.sha256(json.dumps(config,sort_keys=True).encode()).hexdigest()


def check_microduck_runtime(config):
    policy=config.get('policy',{})
    required={'enabled':True,'mode':'walk','action_scale':1.0,'gain':200,
              'head_lowpass':.5,'legs_lowpass':.7,'voltage_adapt':False}
    defaults={'enabled':True,'mode':'walk','action_scale':.9,'gain':200,
              'head_lowpass':.5,'legs_lowpass':.7,'voltage_adapt':False}
    for key,value in required.items():
        if policy.get(key,defaults[key])!=value:
            raise ValueError(f'robotd policy.{key} must equal {value!r} for the captured alpha training controller')


class Network:
    def __init__(self,bundle):
        import onnxruntime as ort
        options=ort.SessionOptions();options.intra_op_num_threads=1
        self.session=ort.InferenceSession(str(bundle/'policy.onnx'),options,providers=['CPUExecutionProvider'])
        self.input=self.session.get_inputs()[0]
        self.source=json.loads((bundle/'provenance.json').read_text())
        if self.input.shape[-1]!=self.source['observations'] or self.session.get_outputs()[0].shape[-1]!=self.source['actions']:
            raise ValueError('ONNX shape disagrees with export provenance')
    def __call__(self,obs):
        value=np.asarray(obs,dtype=np.float32)
        if value.shape!=(self.source['observations'],) or not np.isfinite(value).all():raise ValueError('Invalid measured observation')
        action=self.session.run(None,{self.input.name:value[None]})[0][0]
        if action.shape!=(self.source['actions'],) or not np.isfinite(action).all():raise ValueError('Invalid policy output')
        return action


class Simulator:
    def __init__(self,args):self.args,self.env=args,None
    def connect(self):return dict(robot=self.args.robot,transport='MuJoCo simulator',hardware=False)
    def prepare(self,bundle):
        from interactive_env import load
        import torch
        torch.set_num_threads(2)
        net=Network(bundle)
        env=load(bundle,self.args.root,self.args.robot,42)
        if self.env:self.env.close()
        self.env,self.net,self.dt=env,net,env.dt
    def dry_run(self):
        obs=self.env.observe([0.,0.,0.]);action=self.net(obs)
        for _ in range(3):self.env.step(self.net(self.env.observe([0.,0.,0.])),[0.,0.,0.])
        self.env.reset([0.,0.,0.])
        return dict(passed=True,source='measured simulator rehearsal; reset before arming; no motor writes',observations=len(obs),actions=len(action),max_abs_action=float(np.abs(action).max()))
    def arm(self):self.env.reset([0.,0.,0.])
    def step(self,command):
        obs=self.env.observe(command);action=self.net(obs);state=self.env.step(action,[0.,0.,0.])
        return dict(joint_names=self.env.names,observation=obs.tolist(),action=action.tolist(),**state)
    def stop(self):pass
    def rollback(self):raise ValueError('No previous deployed simulator policy')


class Hardware:
    dt=.02
    def __init__(self,args):
        self.args=args
        if not args.hardware_config:raise ValueError('Hardware requires a target calibration/contract file (--hardware-config)')
        self.cfg=json.loads(args.hardware_config.read_text())
        if self.cfg['robot']!=args.robot:raise ValueError('Target calibration belongs to another robot')
        self.bundle=None
    def prepare(self,bundle):
        if contract_sha(bundle)!=self.cfg['policy_contract_sha256']:
            raise ValueError('Policy observation/actuator/controller contract differs from target calibration')
        self.net=Network(bundle);self.bundle=bundle
    def dry_run(self):
        # No actuator write: measured positions/orientation plus an explicit reset history.
        obs=self.observation([0.,0.,0.]);action=self.net(obs)
        return dict(passed=True,source='live target sensors with reset action history; no motor writes',observations=len(obs),actions=len(action),max_abs_action=float(np.abs(action).max()))
    def rollback(self):raise ValueError('No earlier policy is retained for this target')


class Rpc:
    def __init__(self,path,subscribe=False):
        self.socket=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);self.socket.settimeout(.2);self.socket.connect(str(path))
        self.reader=self.socket.makefile('rb');self.seq=0;self.latest=None
        self.call('hello',dict(api_version=28))
        self.condition=threading.Condition();self.arrived=0.;self.consumed=None;self.stream_error=None
        if subscribe:
            self.call('robot.subscribe',dict(hz=50))
            threading.Thread(target=self.receive_states,daemon=True).start()
    def call(self,method,params=None,notify=False):
        self.seq+=1
        req=dict(jsonrpc='2.0',method=method,params=params or {})
        if not notify:req['id']=self.seq
        self.socket.sendall((json.dumps(req)+'\n').encode())
        if notify:return
        deadline=time.monotonic()+.2
        while time.monotonic()<deadline:
            line=self.reader.readline(1024*1024)
            if not line:raise ConnectionError('robotd disconnected')
            response=json.loads(line)
            if response.get('method')=='robot.state':self.latest=response['params']
            if response.get('id')==self.seq:
                if 'error' in response:raise ValueError(str(response['error']))
                result=response['result']
                if isinstance(result,dict) and result.get('accepted') is False:raise ValueError(result.get('reason','robotd refused command'))
                return result
        raise TimeoutError('robotd request timed out')
    def receive_states(self):
        try:
            while True:
                line=self.reader.readline(1024*1024)
                if not line:raise ConnectionError('robotd sensor stream closed')
                response=json.loads(line)
                if response.get('method')=='robot.state':
                    with self.condition:
                        self.latest=response['params'];self.arrived=time.monotonic();self.condition.notify_all()
        except Exception as error:
            with self.condition:self.stream_error=str(error);self.condition.notify_all()
    def state(self):
        deadline=time.monotonic()+.2
        with self.condition:
            while time.monotonic()<deadline:
                if self.stream_error:raise ConnectionError(self.stream_error)
                if self.latest is not None and self.latest['t']!=self.consumed and time.monotonic()-self.arrived<.15:
                    self.consumed=self.latest['t'];return self.latest
                self.condition.wait(max(.001,deadline-time.monotonic()))
        raise TimeoutError('robotd sensor stream is stale')


class Microduck(Hardware):
    HOME=np.array([0.,-.0873,-.4579,-.0049,.4530,.3491,.3491,0.,0.,0.,.0873,.4579,.0049,-.4530])
    def connect(self):
        self.rpc=Rpc(self.cfg.get('socket','/run/robotd.sock'))
        self.stream=Rpc(self.cfg.get('socket','/run/robotd.sock'),True)
        model=self.rpc.call('robot.model');self.names=model['joint_names']
        from duck_task_config import JOINTS
        if [n for n in self.names if n!='mouth']!=JOINTS:raise ValueError('robotd joint mapping does not match Microduck alpha')
        self.indices=[i for i,n in enumerate(self.names) if n!='mouth']
        self.last=None;self.action=np.zeros(14)
        self.backup=self.args.state/'microduck-policy-backup.json'
        return dict(robot=self.args.robot,model=model.get('asset'),health=self.rpc.call('robot.health'))
    def prepare(self,bundle):
        super().prepare(bundle)
        if (self.net.source['observations'],self.net.source['actions'])!=(61,14):raise ValueError('robotd requires its 61-observation / 14-action alpha policy')
        # robotd owns scaling and actuator gains. Check the locally reviewed configuration
        # instead of silently changing firmware calibration from a remote UI.
        path=Path(self.cfg.get('runtime_config','/etc/robot/robotd.toml'))
        calibration=microduck_calibration(path)
        if calibration_sha(calibration)!=self.cfg['runtime_calibration_sha256']:raise ValueError('robotd calibration changed since target commissioning')
        check_microduck_runtime(calibration)
        if (bundle/'task.json').exists():
            from duck_task_config import preset
            task=json.loads((bundle/'task.json').read_text());base=preset('microduck')
            if task['observations']!=base['observations'] or task['actuators']!=base['actuators']:
                raise ValueError('Edited observations/calibration require a matching robotd controller; refusing the standard alpha runtime')
    def observation(self,command):
        state=self.stream.state();self.sensor=state
        if not state.get('imu'):raise ValueError('robotd must provide measured IMU telemetry')
        q=np.asarray(state['joints'])[self.indices]
        dq=np.zeros(14) if self.last is None else (q-self.last[1])/max(.001,state['t']-self.last[0])
        self.last=(state['t'],q.copy())
        w,x,y,z=state['imu']['quat'];gravity=np.array([2*(w*y-x*z),-2*(w*x+y*z),2*(x*x+y*y)-1])
        return np.r_[state['imu']['gyro'],gravity,q-self.HOME,dq,self.action,command,np.zeros(10)]
    def load_slot(self,slot,path):
        self.rpc.call('robot.loadPolicy',dict(slot=slot,path=path))
        deadline=time.monotonic()+3
        while time.monotonic()<deadline:
            result=self.rpc.call('robot.policies')
            entry=next((s for s in result['slots'] if s['slot']==slot),None)
            if result.get('change_error') or (entry and entry.get('error')):raise ValueError('robotd policy swap failed')
            if entry and (entry.get('path')==path or (path=='none' and entry.get('path') is None)):return
            time.sleep(.05)
        raise TimeoutError(f'robotd did not confirm {slot} policy')
    def save_original(self):
        if self.backup.exists():return
        from agent import atomic
        saved={}
        for entry in self.rpc.call('robot.policies')['slots']:
            slot=entry['slot']
            if slot not in ('walk','stand'):continue
            path=entry.get('path')
            if path in (None,'none'):
                saved[slot]=dict(path='none');continue
            source=Path(path)
            if not source.is_absolute() or not source.is_file():raise ValueError('Original robotd policy path is unavailable')
            digest=hashlib.sha256(source.read_bytes()).hexdigest()
            dest=self.args.state/'original-policies'/f'{slot}-{digest}.onnx'
            dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(source,dest)
            if hashlib.sha256(dest.read_bytes()).hexdigest()!=digest:raise ValueError('Original policy changed during snapshot')
            saved[slot]=dict(path=str(dest.resolve()),sha256=digest,source=str(source))
        if set(saved)!={'walk','stand'}:raise ValueError('robotd did not publish both policy slots')
        atomic(self.backup,saved)
    def arm(self):
        self.rpc.call('robot.stop');self.rpc.call('robot.enable',dict(on=False))
        self.save_original()
        # robotd has one pending swap; acknowledge each before submitting another.
        self.load_slot('walk',str(self.bundle/'policy.onnx'))
        self.load_slot('stand','none')
        self.last=None;self.action=np.zeros(14)
        self.rpc.call('robot.enable',dict(on=True))
    def step(self,command):
        self.rpc.call('robot.move',dict(vx=command[0],vy=command[1],vyaw=command[2]),notify=True)
        state=self.stream.state()
        # robotd owns policy inference; do not fabricate raw observations/actions/torques
        # that its current telemetry protocol does not publish.
        safety=state['safety'];fallen=bool(safety.get('fallen',False))
        pose={}
        if state.get('imu') and state.get('odom'):
            quat=state['imu']['quat'];pose['base']=[state['odom']['position']+quat[1:]+quat[:1]]
        return dict(source='robotd',joint_names=self.names,joints=[state['joints']],targets=state['targets'],**pose,
                    robot_state=state,terminal=fallen,terminal_reason='robotd fall' if fallen else '')
    def stop(self):
        self.rpc.call('robot.stop');self.rpc.call('robot.enable',dict(on=False))
    def rollback(self):
        if not self.backup.exists():raise ValueError('No original robotd policy snapshot')
        saved=json.loads(self.backup.read_text())
        for entry in saved.values():
            if entry['path']!='none' and hashlib.sha256(Path(entry['path']).read_bytes()).hexdigest()!=entry['sha256']:
                raise ValueError('Original robotd policy snapshot changed')
        self.stop()
        for slot,entry in saved.items():self.load_slot(slot,entry['path'])


class Unitree(Hardware):
    def connect(self):
        from unitree_sdk2py.core.channel import ChannelFactoryInitialize,ChannelPublisher,ChannelSubscriber
        from unitree_sdk2py.idl.unitree_hg.msg.dds_ import LowCmd_,LowState_
        from unitree_sdk2py.idl.default import unitree_hg_msg_dds__LowCmd_
        from unitree_sdk2py.utils.crc import CRC
        from unitree_sdk2py.comm.motion_switcher.motion_switcher_client import MotionSwitcherClient
        ChannelFactoryInitialize(0,self.cfg['interface'])
        self.low=None;self.received=0.;self.crc=CRC();self.message=unitree_hg_msg_dds__LowCmd_()
        self.io_lock=threading.Lock();self.active=False;self.desired=None;self.updated=0.;self.io_error=None;self.last_torques=[0.]*29
        self.pub=ChannelPublisher('rt/lowcmd',LowCmd_);self.pub.Init()
        self.sub=ChannelSubscriber('rt/lowstate',LowState_);self.sub.Init(self.receive,10)
        self.switch=MotionSwitcherClient();self.switch.SetTimeout(.2);self.switch.Init()
        deadline=time.monotonic()+2
        while self.low is None and time.monotonic()<deadline:time.sleep(.02)
        self.sensor()
        threading.Thread(target=self.io_loop,daemon=True).start()
        return dict(robot=self.args.robot,transport='Unitree SDK2 DDS',mode_machine=self.low.mode_machine)
    def receive(self,state):self.low,self.received=state,time.monotonic()
    def sensor(self):
        if self.low is None or time.monotonic()-self.received>.15:raise TimeoutError('Unitree lowstate is stale')
        if self.low.mode_machine!=self.cfg['mode_machine']:raise ValueError('Unitree hardware mode differs from commissioned 29-DOF target')
        if len(self.low.motor_state)<29:raise ValueError('Unitree target has fewer than 29 motors')
        return self.low
    def prepare(self,bundle):
        super().prepare(bundle)
        from g1_env import G1Adapter,LEGS
        self.env=G1Adapter(bundle,self.args.root,42)
        self.names=LEGS+[n for n in self.env.names if n not in LEGS]
        if self.names!=self.cfg['joint_names']:raise ValueError('Unitree SDK2 motor order differs from target calibration')
        self.env.reset([0.,0.,0.]);self.ramp=None
        limits=np.asarray(self.cfg['torque_limits'])
        if limits.shape!=(29,) or not np.isfinite(limits).all() or np.any(limits<=0):raise ValueError('Provide 29 finite positive commissioned torque limits')
    def observation(self,command):
        import mujoco
        state=self.sensor();env=self.env
        for i,n in enumerate(self.names):
            env.data.qpos[env.model.joint(n).qposadr[0]]=state.motor_state[i].q
            env.data.qvel[env.model.joint(n).dofadr[0]]=state.motor_state[i].dq
        env.data.qpos[3:7]=state.imu_state.quaternion
        env.data.qvel[3:6]=state.imu_state.gyroscope
        if not np.isfinite(env.data.qpos).all() or not np.isfinite(env.data.qvel).all():raise ValueError('Nonfinite Unitree sensors')
        mujoco.mj_forward(env.model,env.data)
        return env.observe(command)
    def arm(self):
        code,mode=self.switch.CheckMode()
        if code!=0:raise RuntimeError('Cannot read Unitree motion mode')
        if mode.get('name'):raise ValueError('Release the Unitree high-level motion service locally before low-level arming')
        self.env.reset([0.,0.,0.]);self.observation([0.,0.,0.])
        self.ramp=(time.monotonic(),np.array([self.sensor().motor_state[i].q for i in range(29)]))
    def step(self,command):
        from g1_env import DEFAULT,LEGS
        obs=self.observation(command);env=self.env;action=self.net(obs)
        q=env.data.qpos[env.q].copy()
        env.history=([env.pending_frame.copy() for _ in range(env.cfg['history'])] if env.history is None else env.history[1:]+[env.pending_frame.copy()])
        env.phase=env.decision_phase
        targets=dict(zip(LEGS,np.clip(DEFAULT+env.cfg['action_scale']*action,env.ranges[:,0],env.ranges[:,1])))
        gains=dict(zip(LEGS,zip(env.kp,env.kd,env.effort)))
        for qa,va,kp,kd,effort,target in env.held:
            name=env.model.joint(int(env.model.dof_jntid[va])).name;targets[name]=target;gains[name]=(kp,kd,effort)
        fraction=min(1.,(time.monotonic()-self.ramp[0])/3.)
        if self.io_error:raise RuntimeError(self.io_error)
        state=self.sensor();applied=[];control=[]
        for i,n in enumerate(self.names):
            goal=self.ramp[1][i]*(1-fraction)+targets[n]*fraction
            kp,kd,effort=gains[n];limit=min(effort,float(self.cfg['torque_limits'][i]))
            applied.append(float(goal));control.append((goal,kp,kd,limit))
        with self.io_lock:self.desired=control;self.updated=time.monotonic();self.active=True
        env.previous_q=q;env.actions=[env.actions[1],action.copy()];env.count+=1
        tilt=env.data.xmat[env.root_id].reshape(3,3)[2,2]
        return dict(source='Unitree SDK2',joint_names=self.names,joints=[[state.motor_state[i].q for i in range(29)]],
                    observation=obs.tolist(),action=action.tolist(),targets=applied,commanded_torques=list(self.last_torques),
                    terminal=bool(tilt<np.cos(np.radians(65))),terminal_reason='IMU tilt limit' if tilt<np.cos(np.radians(65)) else '')
    def io_loop(self):
        # Match the captured simulator's 5 ms PD update; policy targets arrive at 50 Hz.
        while True:
            t=time.monotonic()
            with self.io_lock:
                if self.active:
                    try:
                        if t-self.updated>.15:raise TimeoutError('Unitree policy target deadline expired')
                        state=self.sensor()
                        self.message.mode_pr=0;self.message.mode_machine=state.mode_machine
                        torques=[]
                        for i,(goal,kp,kd,limit) in enumerate(self.desired):
                            tau=float(np.clip(kp*(goal-state.motor_state[i].q)-kd*state.motor_state[i].dq,-limit,limit))
                            motor=self.message.motor_cmd[i];motor.mode=1;motor.q=0.;motor.dq=0.;motor.kp=0.;motor.kd=0.;motor.tau=tau
                            torques.append(tau)
                        self.message.crc=self.crc.Crc(self.message)
                        if self.pub.Write(self.message) is False:raise ConnectionError('Unitree command write failed')
                        self.last_torques=torques
                    except Exception as error:
                        self.io_error=str(error);self.active=False
                        try:self.disable()
                        except Exception:pass
            time.sleep(max(.0005,.005-(time.monotonic()-t)))
    def disable(self):
        # SDK2 write acceptance is recorded; it is not proof of physical standstill.
        for motor in self.message.motor_cmd:
            motor.mode=0;motor.q=0.;motor.dq=0.;motor.kp=0.;motor.kd=0.;motor.tau=0.
        self.message.crc=self.crc.Crc(self.message)
        if self.pub.Write(self.message) is False:raise ConnectionError('Unitree stop write failed')
    def stop(self):
        with self.io_lock:self.active=False;self.disable()


def make_backend(args):
    if args.adapter=='simulator':return Simulator(args)
    if args.adapter=='microduck':return Microduck(args)
    if args.adapter=='unitree':return Unitree(args)
    if args.adapter=='open-duck':
        from open_duck import OpenDuck
        return OpenDuck(args)
    raise ValueError('Unknown target adapter')
