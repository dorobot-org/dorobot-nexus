# Measured policy evaluation

Select a run/checkpoint and open **Validate → Evaluate / compare…**. Leave the comparison path blank to evaluate one policy, or enter an absolute path to compatible checkpoint B. Choose a duration from 0.2 to 30 seconds and one to eight distinct unsigned 32-bit seeds. Every seed runs stand, forward 0.1 m/s, forward 0.2 m/s, yaw +0.5 rad/s and yaw −0.5 rad/s. Trials stop at a fall or the captured task's episode limit. A shorter episode limit does not count as survival of the requested duration.

G1, Microduck, Open Duck and authored generic MuJoCo tasks use the same captured controller as interactive physics. Each checkpoint is exported with normalized ONNX inference and a retained source checkpoint. Comparisons require the same physical/actuator contract; differing network weights or PPO settings are allowed. Seeds and commands are paired across A and B. This checks the captured flat task, not arbitrary unseen terrain or physical transfer.

The left list selects measured trials and paginates long case sets. The two viewports share a timeline and camera framing; a terminal policy holds its last measured pose. The right rail reports survival, command-tracking passes and x/y/yaw velocity RMSE. Survival uses a Wilson 95% interval; paired tracking differences use 2,000 deterministic bootstrap resamples, reported as B minus A (negative favors B). These intervals describe the sampled command/seed trials, not a guarantee about all deployment conditions.

Tracking passes require full-duration survival and RMSE at most `[0.1, 0.1, 0.3]` for G1, or `[0.05, 0.05, 0.2]` for the other tasks (m/s, m/s, rad/s). Fixed-base generic tasks can actuate correctly while failing locomotion commands. Generic task evaluation does not establish task-specific manipulation success; author that objective and metrics for such applications.

**Cancel** also works while the worker is starting. It preserves diagnostics and partial trajectories without publishing a completed report. **Latest report** reopens the newest completed evaluation after restart and restores its robot/run/checkpoint selection. Invalid report identity, schema or selected trajectory hashes are rejected. **Export report…** writes JSON/CSV/Markdown metrics and measured trajectories to a new absolute folder, preserving existing destinations. Every trajectory and source policy has a SHA-256 digest. Original reports remain under `evaluations/eval…`; captured policy bundles remain there when exporting only the report.

CLI example (same worker as Studio):

```sh
training-stack/microduck_rl/.venv/bin/python tools/training/evaluate_policy.py \
  --checkpoint /absolute/run/policy.pt --compare /absolute/run/training/model_0.pt \
  --robot Microduck --robot-id library-id --run-id run-id \
  --seconds 10 --seeds 10001,20003,30007 --output /absolute/new-report
```

`test_evaluation.py` trains actual MPS weights, recomputes metrics independently from recorded physical velocities, verifies nonzero differences between learned policies and exercises cancellation. The completion report records the existing policies' measured performance; workflow tests do not establish walking readiness.
