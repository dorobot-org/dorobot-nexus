# Open Duck Mini v2 in Nexus Studio

Open Duck has a native MuJoCo task port with RSL-RL PPO on the Apple GPU,
physical training poses, resumable checkpoints, normalized ONNX export, and
measured validation replays. Selecting Open Duck or Microduck chooses that
robot's profile from `training-stack/robots.profiles.json`.

The upstream [Open Duck Playground](https://github.com/apirrone/Open_Duck_Playground)
trainer uses JAX/Brax and declares CUDA dependencies. This Mac adapter is a
**Nexus port**, not an execution of that upstream trainer. The clean checkout
is pinned to `b9be205ac64488c23504ca42e5ec790337adeec3`. Startup rejects a
different revision or tracked source changes.

## Task contract and differences

- The unchanged `scene_flat_terrain.xml` provides the 14-actuator robot,
  contacts, sensors, home pose, motor gains and limits. Physics uses native
  MuJoCo on CPU at 500 Hz; control is 50 Hz. Network parameters, PPO buffers,
  normalization and optimization use PyTorch MPS. Missing MPS fails explicitly.
- The actor has 101 observations: gyro, acceleration, seven commands,
  14 joint-position errors, scaled joint velocities, three past actions,
  motor targets, two foot contacts and two reference-phase values. Actions
  are 14 home-relative targets scaled by 0.25 and limited to 5.24 rad/s.
- Pinned upstream NumPy reward functions and polynomial reference motion are
  used directly. Reward weights, command ranges, reset ranges, random action
  delays, observation noise, pushes and upside-down training termination follow
  the joystick task. The port refreshes derived sensors after each integrated
  control step and applies joint noise by joint name, correcting the upstream
  no-head index list's mismatch with its 14-actuator model.
- The critic is symmetric (the same 101 observations), using normalized
  256/256/128 ELU networks, RSL-RL 5.0.1 PPO, 24-step rollouts and five update
  epochs. This differs from the upstream privileged critic and Brax optimizer.
  This flat-terrain recipe does **not** implement the backlash model or the
  upstream mass/friction/actuator domain randomizer. It is not a sim-to-real
  qualification recipe. The pinned training task's discarded accelerometer
  bias assignment is preserved as no bias; its separate legacy inference
  script is therefore not substituted for this adapter's observation builder.
- The displayed root and fourteen servos are measured physics. The URDF's two
  decorative antenna joints are set to zero because the official RL model
  fixes them. Packets explicitly record `locked_joints`. Camera adjustments
  keep the low robot visible without modifying its physical pose.

## Install and configure

This workspace already has the pinned source and dependencies. It reuses the
existing `training-stack/microduck_rl/.venv/bin/python` interpreter without
changing Microduck's code or locked packages. An independent environment can
also be created:

```sh
git clone https://github.com/apirrone/Open_Duck_Playground.git training-stack/open_duck_playground
git -C training-stack/open_duck_playground checkout b9be205ac64488c23504ca42e5ec790337adeec3
training-stack/bin/uv venv --python 3.12 training-stack/open-duck-venv
training-stack/bin/uv pip install --python training-stack/open-duck-venv/bin/python -r tools/training/requirements-open-duck.txt
```

Configure a new run with the installed interpreter, or pass `--python` for
the independent environment:

```sh
python3 tools/training/configure_open_duck.py --num-envs 32 --iterations 1000 --evaluate-every 100 --microduck-profile training-stack/microduck.mps.continue.profile.json
NEXUS_TRAINING_PROFILE="$PWD/training-stack/robots.profiles.json" cargo run --release -p nexus-studio
```

In **Robots**, select **Open Duck Mini v2**, then **Start training**. The
configured task is displayed next to that control. Select Microduck to use
its existing official trainer. A single active job remains the Studio limit.

To continue a saved Open Duck run, regenerate its profile with
`--resume-checkpoint /absolute/path/to/training/model_N.pt`. Resume verifies
robot/revision/observation metadata, copies the input into the new run, restores
network/normalization/optimizer/learning-rate state, and starts after the saved
iteration. Physics resets afresh; it does not promise a bit-identical RNG
continuation. The iteration budget is **additional** iterations. A frozen copy
of the selected profile is saved in each run.

## Motion, validation and artifacts

**Train** reads the atomic `live-motion.json` stream. The same measured frames
are retained in `training-motion.jsonl`. Simulation time, reset count, stream
freshness and completion are shown in the viewport. Training exploration is
different from deterministic checkpoint evaluation.

The first saved checkpoint, subsequent save intervals and final checkpoint
are exported with normalization and numerically checked against PyTorch.
Each evaluation gets `training/verified.json`, `validation/report.json`,
`validation/report.md`, recorded trial JSON files and `nexus-validation.json`.
The latest manifest is also published as `validation-preview.json` at the run
root. Checkpoint hashes tie each report to its exported policy. The final
`policy.pt` supports resume; `policy.onnx` supports deterministic inference
with this adapter's observation/control contract.

**Validate** offers five commands × three seeds, up to ten seconds each.
Survival requires tilt ≤60° and root height ≥0.07 m throughout. Tracking also
requires per-axis linear velocity RMSE ≤0.05 m/s and yaw RMSE ≤0.20 rad/s.
A fall immediately fails the trial and ends its measured recording. Failed
trials are not reset or padded to look successful. Older/Latest, command/seed,
Play/Pause and timeline controls all use recorded physical poses. Playback
does not modify training or enable hardware deployment.

Re-run an existing exported checkpoint evaluation:

```sh
training-stack/microduck_rl/.venv/bin/python tools/training/validate_open_duck.py --repo training-stack/open_duck_playground --run-dir runs/RUN_ID --evaluation-dir runs/RUN_ID/evaluations/iteration_000160
```

Tests and observed results are recorded in
[the Studio verification note](../../docs/OPEN_DUCK_WIRING.md).
