# Training queue and supervisors

Studio captures each new run's model assets, task/profile, checkpoint inputs and launch settings before saving a queue entry. Training runs in a separate supervisor process. Closing Studio detaches the monitor; opening the same workspace reconnects to the saved job identity. The supervisor's process lock, acknowledgements and exit status determine the displayed state.

In Train, open **Launch settings** to choose local execution or SSH and an optional delay in minutes. **Hold queue**, **Run queue**, **First**, **Run now** and **Cancel** control pending work. The queue dispatches one training job at a time. Studio must be open to dispatch pending jobs; an already launched job continues independently. Pause freezes the worker process group, including its Python children. Stop terminates that group, including a paused worker, before reporting Stopped.

SSH settings require the host/user, remote Nexus repository and run-storage paths, remote `nexus-studio` executable and Python interpreter, plus an optional port, private-key file and known-hosts file. The training dependencies, native binary and robot-specific upstream checkouts must already be installed on that machine. SSH uses batch authentication and verified host keys. The transport installs a content-addressed copy of the standard-library supervisor under the chosen remote run-storage directory.

Captured inputs have SHA-256 checks; Python trainer source files are checked before launch. The remote job receives its own run-storage path. Outputs download in bounded chunks with checksums and consistent file timestamps, then publish by rename. JSON artifact paths are rebased to the local run directory. Completed is not accepted by the monitor until the final transfer succeeds. This captures the job configuration and inputs; installed native binaries and dependency environments are not archived as reproducible images.

Connection loss retains the job identity and retries observation. A lost supervisor is Unknown, since worker termination cannot be inferred from a disconnected monitor or an old PID. Automatic duplicate launch and unverified PID termination are refused. A failed input check is saved as Failed before a training worker starts. Starting or cancelled submissions may be retried safely under the same token.

Launch settings also select **1–64 learner processes on the chosen host**. Authored generic, Open Duck and Microduck tasks support shared-policy PPO on CPU or one CUDA GPU per process. The task's environment count is per process; job progress counts their combined transitions. Apple MPS keeps one learner process and does not silently switch devices. Native G1 and CPU reference backends also use one learner; an authored G1 MuJoCo task can use parallel PPO.

RSL-RL averages policy gradients and adaptive-KL updates. Nexus pools observation-normalization samples, aggregates measured metrics and requires identical final actor, critic and optimizer state across ranks. Rank 0 writes the common policy and exports/validates it; per-rank logs and a `parallel-result.json` retain evidence. All learners remain in the supervised process group, so pause/stop and Studio restart apply to the complete job. A failed rank terminates and reaps the others. Continue and Reproduce retain the captured process count even if defaults have since changed.

Distributed execution was validated with two CPU learners locally and through a real loopback OpenSSH server, including paused-job restart, continuation, transfer and stop. Single-process duck training was rechecked on MPS. This is one-host execution; multi-host orchestration is not offered. Separate-host/CUDA and physical-robot acceptance are not established by these tests.

## Validation

```sh
training-stack/microduck_rl/.venv/bin/python -m unittest discover -s tools/jobs -p 'test_*.py'
cargo test --workspace --all-features --release
```

The SSH integration case is opt-in with `NEXUS_TEST_SSH=1`. Its isolated fixture expects a loopback OpenSSH server on port 22489 with test credentials at `/tmp/nexus-job-ssh/`; it does not disable host verification or change global SSH configuration. Without the fixture, that case is explicitly skipped.

Run `python3 -m unittest discover -s tools/jobs -p 'test_*.py' -v` for local process protocol checks. SSH integration requires an explicitly configured test host.
