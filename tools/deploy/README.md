# Deployment agent

Nexus Deploy exports the selected checkpoint, transfers a hashed controller bundle, checks the target contract, runs a sensor-based dry-run, and exposes explicit arm, velocity stage, stop and rollback controls. Local rehearsal uses the actual captured MuJoCo policy. Hardware adapters target Microduck alpha (`robotd` API 28), Open Duck Mini v2 (official Feetech runtime) and Unitree G1-29dof (SDK2 DDS).

## Rehearse in Studio

Select a run/checkpoint, open **Deploy**, add a target with endpoint `sim://local`, then **Connect → Prepare policy → Dry-run → Arm**. Arming is a separate native confirmation. Each command stage needs three seconds and 50 healthy control steps before advancing. Stages permit 0%, 25%, 50% and 100% of configured velocity limits; they are not torque percentages. Stop leaves the target disarmed. Deploying another policy retains the previous bundle for rollback; rollback also leaves it disarmed and requires another dry-run before arm.

The viewport shows measured joint state. Missing hardware pose/action/torque channels stay unavailable; Microduck's daemon publishes positions and targets but not its policy's internal observation/action arrays. **Evidence** opens the retained session directory.

## Install on a target

Copy this repository's `tools/deploy`, `tools/training` and the exported policy bundle to the target. Use Python 3.11+ with NumPy and ONNX Runtime. Simulator rehearsal and G1's captured controller additionally use the installed Nexus MuJoCo environment; Microduck rehearsal needs the pinned official training stack. Hardware Open Duck uses the official runtime and its pinned dependencies, including `rustypot==0.1.0`; installing an unrelated newer Rustypot API is not compatible.

Create a token file on the target and copy it privately to the Mac. Commands below never print its value:

```sh
python3 -c 'import pathlib,secrets; p=pathlib.Path("agent.token"); p.touch(mode=0o600,exist_ok=False); p.write_text(secrets.token_hex(32))'
```

Prepare a locally reviewed calibration JSON as described below. Bind it to the exported bundle; this command reads files and does not contact hardware:

```sh
python3 tools/deploy/commission.py --adapter microduck --bundle /absolute/export \
  --calibration calibration.json --output commissioned.json
python3 tools/deploy/agent.py --adapter microduck --robot Microduck \
  --state /absolute/nexus-agent-state --token-file /absolute/agent.token \
  --hardware-config /absolute/commissioned.json --port 8091
```

The agent listens only on loopback. From the Mac, use a verified SSH connection:

```sh
ssh -N -L 8091:127.0.0.1:8091 robot-host
```

In Studio add the endpoint `http://127.0.0.1:8091` and the **Mac's absolute token-file path**. HTTPS endpoints behind an authenticated TLS proxy are also accepted. Cleartext non-loopback URLs, redirects and ambient HTTP proxies are rejected. Network-interface, calibration and motor transport setup stay on the robot.

## Target calibration

All configurations capture `robot` and `policy_contract_sha256` through `commission.py`. Any change to observation layout, action mapping, controller or simulation contract requires a new reviewed configuration. Changing PPO budget or checkpoint weights alone does not. The calibrator preserves existing output files.

- **Microduck:** calibration JSON contains `socket` (default `/run/robotd.sock`) and `runtime_config` (default `/etc/robot/robotd.toml`). The standard 61-input/14-output alpha contract is required. The training controller requires `policy.action_scale = 1.0`, gain 200, head/leg low-pass 0.5/0.7, walk mode and voltage adaptation off. The upstream walk default scale is 0.9; commission rejects it rather than silently changing calibration. Apply and verify runtime changes locally using the official project. The semantic TOML hash excludes only policy slot paths, since policy swaps persist those paths. Keep the commissioned daemon version fixed; re-commission after firmware/runtime changes. Edited Nexus observation/actuator contracts need a corresponding robotd implementation and are rejected by this standard adapter. Each walk/stand swap waits for daemon acknowledgement; the original files are copied and hashed before the first arm, so later upstream updates cannot erase rollback. Stop sends zero velocity and disables the policy through robotd; this holds the daemon's home behavior, not a claim of torque-off.
- **Open Duck:** use adapter `open-duck`, robot `Open Duck Mini v2`. Supply absolute `duck_config`, optional `serial_port`, `joint_offsets` copied from that robot's official DuckConfig, 14 ordered `[min,max]` `joint_limits` in radians, and 14 `firmware_kp` / `firmware_kd` values. Order is left leg (yaw, roll, pitch, knee, ankle), neck pitch, head pitch/yaw/roll, right leg. Home pose and offsets must match the captured model. The adapter uses fresh servo/IMU readings, ramps from measured positions over three seconds, clamps targets, and disables torque on stop. Use a supported bench configuration; torque-off is not a balancing maneuver.
- **G1:** use adapter `unitree`, robot `Unitree G1-29dof`. Supply `interface`, measured `mode_machine`, 29 `joint_names` in SDK2 motor order and 29 positive `torque_limits` in N·m. Twelve learned leg actions plus held waist/arm joints follow the captured controller. The adapter consumes actual lowstate/IMU, ramps targets for three seconds and publishes clipped PD torques at 200 Hz with policy targets at 50 Hz. It refuses arming while a high-level motion service is active; arrange the official low-level setup locally. Stop records SDK2 write acceptance, which is not proof of physical standstill.

The supplied policies have not passed walking acceptance (see the completion report). Real target calibration and physical tests are still required. Do not infer them from the simulator or fake-I/O tests.

## Failure and recovery

The agent enforces a monotonic owner sequence, 0.8-second heartbeat deadline by default and independent actuator target/sensor deadlines. The desktop bridge expires when its GUI heartbeat is older than 1.2 seconds. Target control faults and falls trigger stop. Failed stop acknowledgement is displayed as `stop_unconfirmed`; it is never recorded as a successful stop. Agent restart retains bundle history but never resumes motion. A failed initial hardware connection requires restarting the agent after fixing its cause.

Agent state retains hashed bundles, `state.json`, `events.jsonl` and `telemetry.jsonl`. Studio retains export/provenance, bridge logs and status under `deployments/session…`. Rollback checks retained file hashes. The previous policy must be prepared/dry-run/armed again to run it. Microduck can also restore its pre-Nexus daemon policy snapshot when no earlier Nexus deployment exists. Other adapters can roll back after two Nexus policies have been deployed; unknown preexisting controller state cannot be reconstructed.

Tests:

```sh
training-stack/microduck_rl/.venv/bin/python -m unittest discover -s tools/deploy -p 'test_*.py'
```

The HTTP acceptance tests train a temporary generic policy on MPS or CUDA, then use its real exported weights. They explicitly skip on machines with no supported GPU. Hardware tests use recorded/fake I/O and never operate motors. These tests require Python 3.11 or newer and the simulator/training dependencies.

Primary controller references: [Microduck protocol](https://github.com/pollen-robotics/microduck/blob/main/duck-ipc-proto/src/lib.rs), [Microduck observation contract](https://github.com/pollen-robotics/microduck/blob/main/duck-control/src/obs.rs), [Open Duck runtime](https://github.com/apirrone/Open_Duck_Mini_Runtime), [Unitree G1 SDK2 example](https://github.com/unitreerobotics/unitree_sdk2_python/blob/master/example/g1/low_level/g1_low_level_example.py).
