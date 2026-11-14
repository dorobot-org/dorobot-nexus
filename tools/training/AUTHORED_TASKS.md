# Editable robot tasks

In **Robots**, select a robot and choose **Create URDF task**, **Edit official task** (Microduck), or **Edit joystick task** (Open Duck Mini v2). The editor covers PPO, simulation timing, commands, reward weights, observations, and actuator calibration. **Save task** activates a new revision for future runs. A pending draft, including text that has not been applied, is recovered after restart. Cancel retains the draft; Discard removes it.

**Start training** captures the task in the run. **Continue from checkpoint** uses that captured configuration even if the library task has since changed. It restores PPO optimizer and observation-normalization state and adds the requested iteration budget. **Use trainer profile** returns to the installed registry profile. Creating a generic task from a specialized task starts a separate draft; saving it replaces the library's active editable task, while historical run recipes remain intact.

## Task models

| Task | Actual environment | Original actor / action size | Captured assets |
|---|---|---|---|
| Generic URDF | Nexus MuJoCo velocity task | Depends on enabled observations and mapped joints | `model.mjz`, including meshes |
| Open Duck | Nexus NumPy/MuJoCo port of the upstream joystick/imitation task | 101 / 14 | Compiled `model.mjb` |
| Microduck | Original mjlab flat-velocity environment with BAM motor model | 61 / 14 | Unattached robot `model-source.mjz`, including meshes, plus compiled `model.mjb` |

Open Duck uses the pinned reference-motion source and retains its original final reward clipping to [0, 10000]. It is not the upstream JAX/Brax trainer. Microduck retains the original privileged critic observations, motor electronics/friction behavior, and upstream task terms. Its editable observations configure the actor. Both specialized tasks require a floating base and all 14 canonical servo channels; action-slot reassignment swaps channels without changing physical joint identity.

The specialized adapters require their pinned upstream sources and installed dependencies. Captured models make their geometry reproducible; they do not package the whole Python training environment. Open Duck compiled-model continuation also requires the original MuJoCo version. Generic URDF training supplies a general velocity objective, not a robot-specific walking curriculum.

## Microduck calibration and curriculum

- Kp is the firmware proportional gain (default 200), not a MuJoCo PD gain. A separate derivative gain is not part of the BAM motor model.
- Effort cap 0 retains the original BAM limit; a positive cap tightens that limit. Target velocity cap 0 adds no rate limit.
- `upstream` follows the original staged schedules. Edited initial reward weights scale subsequent nonzero-baseline stages; terms whose original initial weight is zero receive an additive offset.
- `hold` keeps the initial stage of each curriculum. `off` disables curriculum changes and uses the edited reward weights as constants.
- Stage times scale with rollout horizon to retain the original iteration cadence. Continuation restores the environment's curriculum counter.

## Devices and measured results

On this Mac, choose `mps`: policy learning runs on the Apple GPU and physics runs on CPU. `cpu` is explicit CPU learning. Microduck can use CUDA for learning and mjlab physics on an available NVIDIA GPU; generic/Open Duck physics remains CPU. An unavailable requested device fails without silently changing the learner.

**Train → Launch settings** can distribute an authored task over multiple CPU learners or CUDA devices on the selected local/SSH host. The editor's environment count is per learner. PPO gradients and observation normalization are shared, and job progress counts all learners' transitions. MPS uses one learner. Continue and Reproduce preserve the captured process count. See [job execution and validation limits](../jobs/README.md).

Training progress reports mean weighted reward per control step for these editable tasks. Fall rate remains **not measured** until an episode outcome exists. Completing a short job before any episode finishes still produces a completed run with actual step counts, checkpoints, export and validation evidence.

Validation evaluates the exported policy and saves command/seed trajectories for **Validate**. Microduck's authored task uses the captured mjlab/BAM play environment; it is distinct from the legacy profile's separate inference-script rehearsal. A completed job is not a successful walking policy. ONNX action parity is not a deployment or hardware approval.

Run the authored-task tests with the relevant simulator environment installed.
