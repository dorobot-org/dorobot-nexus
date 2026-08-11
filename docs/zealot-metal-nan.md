# zealot on Metal: root cause and fix

A single `while` loop in nexus's fused FK/Jacobian/CRBA kernel runs **zero
times instead of once** on Metal. That leaves the DOF chain empty, the body
Jacobian unwritten and the mass matrix never assembled; factorising that
all-zero mass matrix produces NaN, and the whole simulation dies inside one
`env.step()`.

Rewriting the loop as a bounded, fully predicated `for` fixes it. GPU RL
training then runs on Apple Silicon.

The patch is `patches/nexus-metal-jacobian-loop.patch`, applied automatically by
`scripts/setup-zealot.sh`. Nothing is forked — the checkout stays upstream's.

## The bug

`nexus/src_rbd_shaders/dynamics/multibody/compute_dynamics_pre.rs`, in the
chain-building step of the CRBA pass:

```rust
let mut ti = 0u32;
while ti < sd.ndofs {
    chain_buf.write(chain_base + clen as usize, sd.assembly_id + ti);
    clen += 1;
    ti += 1;
}
```

`sd.ndofs` is 1 for a revolute joint, so this loop body should execute exactly
once per link. It is precisely the shape described in zealot's own
`docs/metal-contact-bug-proposal.md`: naga's MSL backend hoists a loop's
`continuing` block and re-evaluates the `break_if` condition *after* the loop
variables have advanced, so a one-iteration loop exits before its body ever
runs.

The consequence is quiet and total: `clen` stays 0, the DOF chain is empty, so
the Jacobian columns are never written, so CRBA never accumulates a mass
matrix, so the LU factorisation of an all-zero matrix yields NaN — and every
pose, velocity and reward downstream is NaN one step later.

This is the same *class* of bug that proposal documents as ✅ RESOLVED
(2026-07-06), and the `naga-fixed` patch that fixed it **is** applied here
(`cargo tree -i naga` resolves to the local path; `src/back/msl/writer.rs`
carries the `break_if` gate). So either that fix is incomplete for this
naga/wgpu pair, or this is a second instance of the same class that the fix
does not cover.

## The fix

```rust
for ti in 0..MAX_JOINT_DOFS as u32 {
    if ti < sd.ndofs {
        chain_buf.write(chain_base + clen as usize, sd.assembly_id + ti);
        clen += 1;
    }
}
```

A constant trip count with a predicated body: no loop condition to lower, no
`break`, so no `break_if` for the MSL writer to mishandle. `MAX_JOINT_DOFS` is
`SPATIAL_DIM`, a correct upper bound (a floating base has 6 DOFs, revolute
joints have 1). The loop appears twice — the Coriolis and non-Coriolis variants
of the kernel — and both are patched.

## Verification

Before and after one `env.step()` from the default spawn, single env:

| buffer | before fix (after 1 step) | after fix (after 1 step) |
|---|---|---|
| torso z | `NaN`, `done=true fell=true` | `0.8203`, `done=false fell=false` |
| body Jacobians | 2649 NaN of 4836, 81 finite non-zero | **0 NaN**, 954 finite non-zero |
| dof_state | 31 NaN of 124 | **0 NaN**, 96 finite non-zero |
| mass matrix (LU) | 935 NaN, **zero** finite non-zero values | **0 NaN**, 949 finite non-zero, max 33.34 |

The zero-action survival probe now behaves as zealot documents. `docs/train-on-macos.md`
says *"passive_stand falls over at ~step 40 → expected on every backend (a
zero-action PD hold is not a stable stand)"*:

```
t=0  torso z env0=0.820   t=5  0.781   t=20 0.767   t=30 0.695
RESULT: mean first-episode len 44.5 steps (0.89s)
```

The torso sags smoothly under gravity instead of exploding, and falls at ~44
steps against a documented ~40.

Training produces real numbers and a live PPO update:

```
iter   curr   step_rew    falls   torso_z         lr       kl     sec
   0   1.00    -0.2788      325     0.760    6.67e-4   0.0245     3.5
  10   1.00    -0.3064      515     0.765    1.32e-4   0.0215     3.5
  20   1.00    -0.3802     1726     0.819    1.73e-5   0.0191     3.5
  29   1.00    -0.4166     2199     0.829    5.85e-5   0.0151     3.5
```

Finite reward, real fall counts, finite KL with the adaptive learning rate
responding to it — 3.5 s/iteration at 256 envs. Previously every one of these
was `NaN` with all 6144 samples terminating.

And it learns. 600 iterations at 512 envs on flat ground, ~5 s/iteration:

```
iter 100  −0.4108   falls 5674   ← trough
iter 200  −0.3837   falls 5571
iter 300  −0.3336   falls 5100
iter 400  −0.2279   falls 3758
iter 500  −0.1523   falls 2763
iter 599  −0.0956   falls 2012   curriculum 0.00 → 0.40
```

Reward up 77% from the trough and falls down 65%, monotonic, while the
curriculum was *raising* difficulty throughout.

The aggregate curve understates it, which is worth noting because it nearly led
to the wrong conclusion here. Between iter 0 and 100 total reward fell while
almost every behavioural term improved — `track_lin_vel` 0.026 → 0.081,
`upright` 0.018 → 0.034, `foot_slip` −0.062 → −0.012, `action_rate_rate`
−0.350 → −0.055 — and the total was dragged down entirely by the `termination`
penalty growing to −0.92 as a more active policy terminated more often. A single
reward curve would have read as "not learning"; the per-term decomposition
showed otherwise.

The behaviour transfers. Same command, same 4-second rollout, flat ground:

| policy | resets / frames | survival |
|---|---|---|
| 30 iterations | 93 / 201 | 54% |
| 600 iterations | 33 / 201 | **84%** |

## How it was found, and the trap that hid it

The failure looked like it did not exist. **zealot resets on termination
*before* recording the next frame**, so a robot that falls on step one still
writes a clean, finite, upright trajectory — the spawn pose, over and over. A
1-second `biped_drive` rollout gave **51 frames with 50 resets** while every
value was finite and the base height sat steady at 0.82.

The test that exposes it: run the same rollout with a trained policy and with
all-zero actions. Byte-identical output means the policy has no effect and
every frame is a reset. Check the `resets` array in the rollout JSON, or count
`FELL` lines on stdout, before trusting any rollout.

Localisation was then a matter of walking the pipeline in order —
Jacobians → mass matrix → dof_state — and finding the first buffer to hold a
non-finite value. The Jacobians were first, and their first 12 entries were the
correct floating-base identity block `[1,0,0,0,0,0, 0,1,0,0,0,0]`, which said
the kernel ran but stopped writing almost immediately.

Ruled out by experiment along the way, all still NaN: env count 1–8, template
count 1–32, explicit `reset_env_to_default_template`, with/without
`initial_obs`, `pin_command_for`, action magnitude 0/0.01/0.1, `BIPED_TERRAIN=0`,
`BIPED_SPAWN_DR=0`, `BIPED_DR=0`, `BIPED_RESET_VEL=0`, `BIPED_DECIMATION=1`,
`BIPED_CUTILE_GEMM=0`, `NEXUS_SMALL_SORT`, wgpu's `force_loop_bounding`,
module inclusion, and `cargo run` vs direct execution. Layers below zealot pass
their own GPU tests here (vortx 14/14, nexus_rbd3d 5/5).

## Environment

| component | pin |
|---|---|
| zealot | `master` @ ab42c46 |
| nexus | `feat/browser-web-demo` @ d2451e0 + this patch |
| khal | `khal/unified` @ 1afd85c |
| vortx | `vortx/unified` @ 1495fe4 |
| parry | `rebase/nexus-0.4` @ 8436f7c (0.29; **not** `spirv-compat`, which is 0.26.1 so the patch silently no-ops) |
| naga-fixed | `vendored-20260805` @ a8be640 |
| cutile-rs | `v0.2.0` @ de6c5cd |

macOS 25.5, Apple Silicon, `[khal] backend = WebGPU`. `cargo-gpu` from
`Rust-GPU/cargo-gpu` — the crates.io crate of that name is a placeholder that
prints "Coming Soon" and exits 0, so the shader build silently emits nothing.
`Cargo.lock` must contain no `[[patch.unused]]`: a version-mismatched patch is
dropped in silence.

## Reproducing the measurement

Add a reader for the Jacobian buffer to zealot's env (it exposes dof_state and
the mass matrix already, but not this):

```rust
pub async fn dbg_jacobians_diag(&mut self) -> Vec<f32> {
    let buf = self.state.multibodies_mut().dbg_body_jacobians().buffer();
    let mut v = vec![0f32; buf.len()];
    self.gpu.slow_read_buffer(buf, &mut v).await.expect("jacobian readback");
    v
}
```

then, as `src/bin/dorobot_diag.rs`:

```rust
#[path = "../biped/biped_env_nexus.rs"]
mod biped_env_nexus;

use biped_env_nexus::{BipedNexusBatchEnv, default_mjcf_path};
use zealot_env::robots::NUM_JOINTS;

fn summarise(name: &str, v: &[f32]) {
    let nan = v.iter().filter(|x| x.is_nan()).count();
    let nz = v.iter().filter(|x| x.is_finite() && **x != 0.0).count();
    println!("  {name:<12} len={:<6} nan={nan:<6} finite_nonzero={nz}", v.len());
}

fn main() {
    let xml = std::fs::read_to_string(default_mjcf_path()).expect("read mjcf");
    pollster::block_on(async {
        let mut env = BipedNexusBatchEnv::new(&xml, 1, 1, 0xC0FFEE).await;
        let _ = env.reset_env_to_default_template(0).await;
        env.pin_command_for(0, 0.0, 0.0, 0.0);

        println!("BEFORE: torso z = {:?}", env.torso_heights().await);
        let outs = env.step(&[[0.0f32; NUM_JOINTS]]).await;
        println!("AFTER:  torso z = {:?} done={} fell={}",
                 env.torso_heights().await, outs[0].done, outs[0].fell);

        summarise("jacobians", &env.dbg_jacobians_diag().await);
        let (dof, mm) = env.dbg_mb_dof_state_and_lu().await;
        summarise("dof_state", &dof);
        summarise("mass_matrix", &mm);
    });
}
```

`BIPED_SPAWN_DR=0 cargo run --release --bin dorobot_diag --features "gpu biped_gpu"`.

## Suggested upstream fix

Two options, in order of preference:

1. Apply this patch (or the equivalent) in nexus. It costs nothing on backends
   that were already correct — a constant-trip-count loop over ≤6 with a
   predicated body — and removes the dependency on how any backend lowers a
   loop condition.
2. Audit `naga-fixed`'s `break_if` gate against this case. The fix is applied
   here and this loop is still miscompiled, so the gate does not cover it.

## A second, quieter instance

The same file's integration loop is miscompiled too, and it is worth separating
from the first because it fails differently:

```rust
let mut k = lane;
while k < num_links { integrate_link(…, k, …); k += t; }
```

This one runs several iterations, so dropping the final one is not fatal — the
last link assigned to each lane simply never integrates, every step. No NaN, no
crash, just a quietly wrong simulation. That is why it survived the first round
of testing: everything looked healthy.

Measured with the zero-action probe, 32 envs, fixed seed, mean first-episode
length:

| build | runs | mean |
|---|---|---|
| `while` | 43.0, 43.0, 43.1 | **43.0** |
| bounded `for` | 54.2, 54.3 | **54.3** |

Run-to-run noise is ±0.1 steps, so an 11-step gap is ~100× the noise: real and
reproducible, a 26% change in how long the robot stays up. Which figure is
*physically* correct cannot be settled here without a CUDA reference — what can
be said is that integrating every link is what the source says it does, and
that zealot's "~step 40" expectation comes from a document that predates the
current default robot (its healthy spawn height, 0.718, does not match today's
0.842).

Both sites are in the shipped patch.

### Three other instances that do not matter here

`impulse_joint_constraints/kernels.rs` has three grid-stride
`while i < cap { … }` loops of exactly the catastrophic one-iteration shape.
They are **not** patched, because this robot never exercises them: zealot builds
its `ImpulseJointSet` empty and never inserts into it (zero insertions), so
`cap` and `len` are zero and the loops iterate zero times by design. They remain
a hazard for any scene that does use impulse joints.
