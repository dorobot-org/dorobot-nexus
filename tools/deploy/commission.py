"""Bind a reviewed hardware configuration to one exported policy controller contract.

No connection or actuator write occurs. Hardware calibration values must be supplied
from the target, not inferred from a simulator's joint limits.
"""
import argparse
import json
import math
from pathlib import Path
from backends import contract_sha, microduck_calibration, calibration_sha, check_microduck_runtime


def bind(bundle, adapter, calibration):
    provenance=json.loads((bundle/'provenance.json').read_text())
    config=dict(calibration)
    expected={'microduck':'Microduck','unitree':'Unitree G1-29dof','open-duck':'Open Duck Mini v2'}[adapter]
    if provenance['robot']!=expected:raise ValueError(f'{adapter} requires {expected}')
    if adapter=='microduck':
        runtime=microduck_calibration(config.get('runtime_config','/etc/robot/robotd.toml'))
        check_microduck_runtime(runtime)
        config['runtime_calibration_sha256']=calibration_sha(runtime)
    else:
        required={'unitree':('interface','mode_machine','joint_names','torque_limits'),
                  'open-duck':('duck_config','joint_offsets','joint_limits','firmware_kp','firmware_kd')}[adapter]
        for field in required:
            if field not in config:raise ValueError(f'Missing measured target calibration: {field}')
        if adapter=='unitree':
            if len(config['joint_names'])!=29 or len(set(config['joint_names']))!=29:raise ValueError('Provide ordered 29 SDK2 joint names')
            values=config['torque_limits']
            if len(values)!=29 or any(not math.isfinite(v) or v<=0 for v in values):raise ValueError('Provide 29 positive torque limits')
        else:
            limits=config['joint_limits']
            if len(limits)!=14 or any(len(v)!=2 or not all(math.isfinite(x) for x in v) or v[0]>=v[1] for v in limits):raise ValueError('Provide 14 ordered joint limits')
            for name in ('firmware_kp','firmware_kd'):
                if len(config[name])!=14 or any(not math.isfinite(x) or x<0 for x in config[name]):raise ValueError(f'Provide 14 finite nonnegative {name} values')
    config.update(robot=expected,policy_contract_sha256=contract_sha(bundle))
    return config


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle',type=Path,required=True)
    parser.add_argument('--adapter',choices=['microduck','unitree','open-duck'],required=True)
    parser.add_argument('--calibration',type=Path,required=True,help='Locally reviewed JSON; see README.md')
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    result=bind(args.bundle.resolve(),args.adapter,json.loads(args.calibration.read_text()))
    with args.output.open('x') as file:json.dump(result,file,indent=2);file.write('\n')
    print(f'Wrote {args.output}; no hardware was contacted')


if __name__=='__main__':main()
