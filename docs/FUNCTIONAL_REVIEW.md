# Nexus Studio setup and workflows

## Completed workflows

| Area | Changes |
| --- | --- |
| Startup | Production starts with templates and real persisted artifacts, without synthetic running jobs or deployment evidence. Fixtures require `--demo`. |
| Training | Launch snapshots include robot identity and the full scene configuration. Native G1 runs use the installed Zealot trainer; the cart-pole CPU learner requires explicit selection. Missing G1 dependencies produce a failure instead of silently changing robots. |
| Job control | A job owns its run ID, metrics and checkpoint destination. Concurrent launch/sweep requests cannot overwrite the active job. CPU pause/resume preserves optimizer state; Stop reaches the actual worker. External processes report unsupported pause rather than pretending to pause. |
| Checkpoints | Independent run directories, validated finite weights/manifests, atomic file publication, final checkpoints for short/stopped CPU jobs and current scores. CLI runs also use fresh run IDs. Existing output paths are refused. |
| Process failures | Exit status, absent checkpoints, missing/non-finite metrics and non-increasing steps reach the UI. Logs persist beside artifacts. Stop/drop terminates managed external process groups; closing stdout cannot make Stop deadlock. |
| Review and validation | Preview, sweep and sim-to-sim resolve the selected artifact. They cannot silently evaluate a global checkpoint from another run. Replay pacing uses the recorded `dt`; the inspector reads recorded poses. Saving a replay writes actual motion to the recording library. |
| Persistence | Scenes, sets, robots, runs, recording metadata and the selected external training profile survive restart. Interrupted runs restore as stopped. Corrupt/unknown workspace documents are preserved for recovery. |
| Robot import | URDF topology, joint references and limits are checked. Referenced meshes/textures are copied into the library and paths rewritten. Import reports static checks; it does not invent physics, actuator mapping or deployment passes. |
| Other RL robots | A per-robot external trainer profile launches the chosen executable directly, snapshots its configuration, ingests JSON metrics and tracks its checkpoint. The profile must identify the selected robot; importing a URDF alone does not select a training algorithm. |
| Deployment | Exported policy bundles and target settings support the robot agent workflow. Hardware operation requires the matching SDK, controller and calibrated setup. |

## Run

Run from this repository; the shared `../dorobot-studio/crates/dorobot-ux` checkout is required.

```sh
# Native Unitree G1 backend (Metal/CUDA support depends on the installed stack):
scripts/setup-zealot.sh
python3 tools/fetch_robot_meshes.py g1
cargo run --release -p nexus-studio

# Explicit CPU cart-pole learner:
NEXUS_BACKEND=cpu cargo run --release -p nexus-studio

# Optional independent MuJoCo validation:
scripts/setup-mujoco.sh
```

Choose a scene set, launch training, inspect metrics, then select its saved checkpoint for preview or validation. CPU cart-pole checkpoints are not G1 policies and cannot run in the G1 MuJoCo evaluator. G1 curriculum launches use 256 environments, 400 iterations per scene and six rounds; this is a sustained training workload. The CLI can run a short isolated check:

```sh
NEXUS_BACKEND=cpu cargo run --release -p nexus-studio -- --headless 10000 --json
```

The initial JSON event contains the unique run ID. `NEXUS_RUN_ID` can supply a fresh explicit ID; `NEXUS_RUNS_DIR` changes artifact storage. `DOROBOT_NEXUS_DIR` changes the studio workspace/assets root. Run from the repository root so the existing Zealot/MuJoCo setup paths resolve. CPU pause/resume applies while the same process remains open; reopening a stopped run is not optimizer recovery.

## External robot trainer contract

In **Robots → Training → Backend…**, select a JSON profile, or set `NEXUS_TRAINING_PROFILE`. The adjacent Unitree G1 and CPU cart-pole buttons switch back to built-in backends. Backend selection persists, and explicit environment settings override a restored selection. See `tools/training/profile.example.json`. Replace its example paths with the actual Isaac Lab, MuJoCo or other trainer wrapper. Arguments are passed directly, without a shell. The `robot` string must exactly match the imported robot name.

`{run_dir}` and `{seed}` are substituted in arguments, environment values and checkpoint path. The checkpoint must be a new absolute path inside `{run_dir}`. The full scene snapshot is written to `{run_dir}/run.json`; the wrapper must read and apply it to its own simulator/task. The adapter cannot infer reward design, observations, action mapping or simulator assets from a URDF.

Emit newline-delimited JSON on stdout. Other text is retained as logs. Metric events must include finite values and strictly increasing unsigned steps:

```json
{"event":"sample","step":4096,"reward":1.2,"fall_rate":0.05,"steps_per_sec":82000,"episode_len":180}
```

Write the checkpoint at the declared path and exit zero. Otherwise Studio marks the run failed and points to its logs. Stop terminates the process group. Pause/resume is available only for the in-process CPU learner; external trainers need their own checkpoint/resume implementation. For replay import, use JSON with positive `dt`, `joint_names`, equally sized `joints` rows, and optional matching `base` rows `[x,y,z,qx,qy,qz,qw]`.

## Validation

```sh
cargo test --locked --workspace --all-features
python3 -m unittest discover -s tools/jobs -p 'test_*.py' -v
```

External trainer and simulator tests require their separately installed Python
environments. Generated workspaces, robot libraries, recordings and checkpoints
are local data and are excluded from version control. The portable Adora Nano
input model is available in `data/adora_nano`; import it through **Robots → Import**.

## Remaining limits

No physical robot acceptance is included with this source snapshot. Hardware
transport, emergency-stop behavior and actuator calibration require validation
on the actual rig. Imported robots need a matching simulation task and actuator
mapping; URDF static checks cannot establish training dynamics or policy quality.

G1 and external robot training require separately installed backends and assets.
Workspace persistence supports one Studio writer per workspace. See the training
and deployment tool guides for each backend's environment and supported controls.
