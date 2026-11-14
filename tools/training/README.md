# Duck training in Nexus Studio

Both imported ducks have training, live physical poses and measured checkpoint
replays. For **Open Duck Mini v2**, see [the Mac PPO adapter](OPEN_DUCK.md).
For native editing of both specialized tasks and generic URDF tasks, see
[editable robot tasks](AUTHORED_TASKS.md). The profile workflow below remains available.
`training-stack/robots.profiles.json` selects the matching trainer when the
robot changes, while existing single-profile files still work. Choose this
registry with **Robots → Backend…** and use **Start training** on the robot page.

## Microduck

Nexus runs the official `Mjlab-Velocity-Flat-MicroDuck` PPO task from
[pollen-robotics/microduck_rl](https://github.com/pollen-robotics/microduck_rl),
pinned to `53b8971b61baf5b7f3c16d135dd7cac37623de4b`. It uses the upstream
MuJoCo Warp environment, BAM XL330 actuators, rewards, domain randomization,
61-element actor observation and 14-element action layout. The imported viewer
URDF is not the training model; upstream selects its walking MJCF.

## Current workspace

The upstream checkout and locked Python 3.12 environment are installed under
`training-stack/microduck_rl`. Three profiles can be generated:

```sh
python3 tools/training/configure_microduck.py --device cpu --num-envs 64 --iterations 1000
python3 tools/training/configure_microduck.py --cpu-smoke
python3 tools/training/configure_microduck.py --device cuda --num-envs 4096 --iterations 6000
```

In Nexus, select **Robots → Microduck → Backend…**, choose the generated JSON
profile in `training-stack/`, then start training. For this MacBook's GPU, use:

```sh
python3 tools/training/configure_microduck.py --device mps --num-envs 64 --iterations 1000
```

Select `microduck.mps.profile.json`. PPO's actor, critic, rollout buffers and
optimizer use PyTorch MPS on the Apple GPU. MuJoCo Warp physics stays on CPU
because Warp's macOS backend does not support Metal. Device placement is checked
against the policy's real parameter device and recorded with each metric, together
with MPS memory usage. Export is performed on CPU through the upstream exporter.

The backend selection is saved
in `studio-workspace.json`. Training uses its named task configuration; G1 scene
sliders are not applied to Microduck.

The CPU and MPS learning paths were verified on this Apple Silicon Mac with the same upstream
training stack. It is slower and the local 64-environment run collects much less
experience per iteration than the documented 4096-environment CUDA recipe.
The CUDA profile must run on a Linux NVIDIA GPU host. This integration does not
allocate cloud resources or transfer jobs to another machine automatically.

## What each run does

1. Check the upstream commit and clean tracked source, then `uv sync --locked`.
2. Run the project's required smoke test: 64 environments, 5 PPO iterations.
3. Export the smoke checkpoint through upstream `run_export` (the implementation
   behind `scripts/export.py`), preserving observation normalization. Verify
   ONNX `[1,61] → [1,14]` and finite outputs before continuing.
4. Train a fresh policy for the profile's environment and iteration budgets.
5. Save periodic PPO checkpoints and export the final normalized policy to ONNX.

Nexus receives real episode return, episode length, steps, throughput and the
fraction of completed episodes terminated before their time limit. For the
walking task, the latter is shown as the fall rate; it includes invalid-state
terminations. Smoke progress is logged separately from the longer run.

Artifacts live in `runs/<run-id>/`:

- `microduck-plan.json`, `training-profile.json`, `run.json`: exact run setup.
- `metrics.jsonl`, `metrics.stderr.log`: structured telemetry and upstream logs.
- `smoke/verified.json`: smoke training and export evidence.
- `training/model_*.pt`, `policy.pt`: periodic and latest PPO checkpoints.
- `policy.onnx`, `training/verified.json`: final normalized export and evidence.
- `training/params/`: upstream environment and agent configurations.

Stop in Nexus terminates the local process group and keeps completed checkpoint
files. This external backend does not support in-memory pause/resume. A short
successful run establishes that training and export work; it does not establish
a reliable walking gait or sim-to-real readiness. Use upstream
`scripts/infer_policy.py` to rehearse a trained ONNX before hardware deployment.

## Verification on 2026-09-09

The initial CPU smoke run completed 7,680 environment steps and exported a finite
61-input/14-output policy. Joint/observation checks ran against the actual
upstream environment. The policy still fell after short episodes, as expected
for five learning iterations. Nexus's 23 studio tests passed, including trainer
error propagation and external budget checks.

The MPS smoke run also completed 7,680 environment steps and exported a finite
ONNX policy. Telemetry confirmed `learner=mps:0`, `physics=cpu`, and approximately
11.4 MB of live MPS allocations during PPO updates. Both checks used 64 environments
and five real learning iterations; neither represents a trained walking gait.

Upstream guidance: [training commands and model conventions](https://github.com/pollen-robotics/microduck_rl),
[smoke test and training operations](https://github.com/pollen-robotics/microduck_rl/blob/develop/AGENTS.md).

## Validate the exported Microduck policy

Use the locked environment to run the official BAM deployment controller in a
bounded, headless evaluation. The script checks the policy hash against the
run's export provenance and requires the clean pinned upstream source.

```sh
training-stack/microduck_rl/.venv/bin/python tools/training/validate_microduck.py \
  --repo training-stack/microduck_rl --run-dir runs/<run-id> --render
```

This runs standing, 0.1/0.2 m/s forward, and ±0.5 rad/s turning commands, each
for 10 seconds from three initial poses. It writes `validation/report.md`,
`report.json`, state traces, Nexus replay JSONs, and videos. Survival and command
tracking are evaluated separately; remaining upright while ignoring commands
fails locomotion validation. The report states the local tracking tolerances
and test limits. Rendering uses the actual upstream MuJoCo model.

The completed MPS run `run1788995658850` (1,000 iterations, 64 environments,
1,536,000 steps) survived all 15 trials, but passed command tracking only for
the three zero-command trials. At a requested 0.2 m/s it averaged approximately
0.0008 m/s; turning commands also produced almost no motion. Inspection of the
rendered sequences showed planted feet. This checkpoint demonstrates nominal
standing, not a walking gait. Continue learning and evaluate later checkpoints
before attempting robustness or hardware tests. Nexus's existing G1 validation
backend is not applicable to this Microduck ONNX.

## Continue learning after the first validation

The continuation profile resumes the final native checkpoint, including actor,
critic, observation normalizers, Adam state, learning rate and curriculum step
counter. It copies the source checkpoint into the new run's `input/` directory
and records its SHA-256. The new run's iteration and step counters measure only
new training; telemetry also records the total PPO iteration. Fresh simulator
episodes are created; this is not an exact replay of the original RNG/physics
state. The smoke test resumes the same input, and its updates are discarded
before the main continuation starts from that input.

```sh
python3 tools/training/configure_microduck.py \
  --device mps --num-envs 256 --iterations 2000 \
  --resume-checkpoint runs/run1788995658850/training/model_999.pt \
  --hold-early-curriculum --evaluate-every 250 \
  --output training-stack/microduck.mps.continue.profile.json
```

This stage collects 12,288,000 additional transitions. The explicit
`--hold-early-curriculum` option retains only the initial stage of each of the
seven upstream flat-ground curricula: action-rate weight -0.1, standing fraction
0.02, small head/body command ranges, 3 mm trunk/head CoM randomization and zero
head-pose-bias penalty. Reward definitions, BAM actuators, observation/action
layout, noise and other randomization remain upstream. This deliberately delays
hardening while walking is learned; it is an adaptation of the original recipe,
not an unchanged replay. Actual overrides are saved in each phase directory.

`--evaluate-every 250` sets the upstream checkpoint interval. Each checkpoint
(including the first resumed checkpoint and final save) is exported with the
official normalized exporter and evaluated in a separate CPU process. Results
live under `evaluations/iteration_<total>/validation/`; the trainer emits a
`validation` event with survival and command-tracking counts. A behavioral failure
continues learning; a broken export/evaluation process fails the job with a log.
These periodic checks do not render video. Use
`evaluate_microduck_checkpoint.py --repo ... --checkpoint ... --output ... --render`
for visual review of a selected checkpoint. Simulation validation runs on CPU;
PPO stays on the Mac GPU. Training waits while each checkpoint is evaluated.

## Live training and validation in the viewport

**Train** now consumes `live-motion.json` from the external run. The worker
publishes the actual physical positions of the 14 named joints and the floating
base of environment zero, in the scene's local coordinates. Root quaternions
are converted from MuJoCo `wxyz` to the viewer's `xyzw`. The display shows the
simulation timestamp and reset count, and marks stale or finished streams.
It includes PPO's sampled actions and domain randomization; this motion is not
a deterministic walking evaluation. Frames are published at up to 20 Hz of wall
time, at the speed training actually runs, and saved in `training-motion.jsonl`.

**Validate** consumes each completed evaluation's `nexus-validation.json`.
Use Newer/Older/Latest to choose a checkpoint, then select a command and seed.
The actual saved rollout drives the same joint/base renderer at the recorded
simulation rate. Pause and scrub controls operate on the recording. The right
rail shows that test's requested/achieved velocities, tilt and measured outcome.
The Live training button returns to the current pose stream. Validation playback
does not change the running PPO job or grant any deployment approval.

The UI checks run/robot identities, named-joint dimensions, finite values,
quaternion norms and validation file locations before applying a pose. Changing
the selected robot/run clears the previous motion. Incomplete validation files
are never published; the latest manifest and live pose use atomic replacement.
Older worker processes do not acquire this hook until their next launch.
