# zealot on Metal: the multibody mass matrix is written as NaN in one step

Report prepared from dorobot-nexus, which drives zealot's GPU trainer as a
subprocess. It is written for upstream (haixuanTao/zealot + dimforge nexus) and
records what was measured, what was ruled out, and what could not be settled
without a CUDA reference.

## Summary

On Apple Silicon / Metal (khal → WebGPU), a single `env.step()` turns a
verified-good initial state into NaN. The multibody mass-matrix / LU buffer is
the first thing observed to be NaN, and its inputs are finite, so the failure
appears to originate in the mass-properties / LU path rather than downstream.

Training therefore reports NaN rewards and 100 % terminations, and no policy can
learn.

## Environment

| component | pin |
|---|---|
| zealot | `master` @ ab42c46 |
| nexus | `feat/browser-web-demo` @ d2451e0 |
| khal | `khal/unified` @ 1afd85c |
| vortx | `vortx/unified` @ 1495fe4 |
| parry | `rebase/nexus-0.4` @ 8436f7c (0.29; **not** `spirv-compat`, which is 0.26.1 and makes the patch a silent no-op) |
| naga-fixed | `vendored-20260805` @ a8be640 |
| cutile-rs | `v0.2.0` @ de6c5cd |

Host: macOS 25.5 (Darwin), Apple Silicon. `[khal] backend = WebGPU`.
`cargo-gpu` installed from `Rust-GPU/cargo-gpu` (the crates.io crate of that
name is a placeholder that prints "Coming Soon" and exits 0, so the shader build
silently emits nothing). 122 SPIR-V modules compile. `Cargo.lock` contains no
`[[patch.unused]]` entries — verified, because a version-mismatched patch is
dropped silently.

The naga patch is confirmed applied: `cargo tree -i naga` resolves to the local
`naga-fixed` path, and `src/back/msl/writer.rs` carries the `break_if` gate.

## Measurement

One env, one template, reset to the default scene via
`reset_env_to_default_template(0)`, command pinned to zero, then a single
`env.step()` with zero actions. Buffers read with
`dbg_mb_dof_state_and_lu()` either side of that step.

```
BEFORE step:  torso z = [0.82199997]
  dof_state    len=124   nan=0     zero=59    max|finite|=0.100000
  mass_matrix  len=961   nan=0     zero=961   max|finite|=0.000000
  contacts     manifold_lens=[0]  total=0

AFTER one step: torso z = [NaN]  done=true  fell=true
  dof_state    len=124   nan=31    zero=28    max|finite|=0.100000
  mass_matrix  len=961   nan=935   zero=26    max|finite|=0.000000
  contacts     manifold_lens=[0]  total=0
```

Reading of this:

- The initial state is correct — torso z is the expected spawn height and
  `dof_state` is entirely finite.
- The mass-matrix / LU buffer **changes** across the step (all-zero → 935 NaN),
  so it is genuinely written during the step; what is written is NaN.
- It never holds a finite non-zero value at any point: `max|finite|` is
  `0.000000` both before and after.
- `dof_state` is finite going in and 31/124 NaN coming out.

The contact readings are reported but **not** relied on: `debug_contact_impulses()`
is `unimplemented!()` on this branch ("probe not ported"), so it could not be
established whether `dbg_contacts` is populated here or simply never written.
A zero contact count is therefore not evidence of a narrow-phase failure.

## Why this is easy to miss

`biped_drive` looks healthy on this machine and is not. It resets on
termination **before** recording the next frame, so a robot that falls on step
one still produces a clean, finite, upright trajectory — the spawn pose,
recorded repeatedly. A 1-second rollout gave **51 frames with 50 resets** while
every recorded value was finite and the base height sat steady at 0.82.

The cheap test that exposes it: run the same rollout with a trained policy and
with all-zero actions. On this machine the two outputs are **byte-identical**,
which means the policy has no effect and every frame is a reset.

## Ruled out by experiment

All of the following still produce NaN after one step:

- env count 1, 2, 4, 8
- template count 1, 2, 8, 32
- explicit `reset_env_to_default_template` before stepping, and without it
- with and without `initial_obs()`
- `pin_command_for` (which also disables command resampling), and without it
- action magnitude 0.0, 0.01, 0.1
- `BIPED_TERRAIN=0`, `BIPED_SPAWN_DR=0`, `BIPED_DR=0`, `BIPED_RESET_VEL=0`
- `BIPED_DECIMATION=1` (note: this asserts unless `BIPED_MOTOR_DELAY` is
  lowered to match — `need min <= max <= decimation`)
- `BIPED_CUTILE_GEMM=0`
- `NEXUS_SMALL_SORT=1` set and unset
- running the binary directly vs through `cargo run` (which injects
  `.cargo/config.toml`'s `[env]`)
- compiling `mod biped_env` into the binary or not

Layers below zealot pass their own GPU tests on this host: `vortx` 14/14
(including `linalg::reduce::test::gpu_reduce_webgpu`) and `nexus_rbd3d` 5/5.

## Relationship to the documented naga bug

`docs/metal-contact-bug-proposal.md` is marked ✅ RESOLVED (2026-07-06), root
caused to naga's MSL backend re-evaluating a loop's `break_if` after the
continuing block advanced the phis, so rust-gpu `while` loops exited one
iteration early and the per-lane `J·v` reductions ran zero times.

The present symptom matches that family — a degenerate multibody solve
producing NaN — but the documented fix is applied. So either it is incomplete
for this naga/wgpu pair, or there is a second miscompile of the same class.

Note also that `docs/train-on-macos.md` §4 gives the verification
(`contact_probe`, healthy `torso_z=0.718`, step-0 impulse `1.697e-1`), but that
example no longer exists after the restructure, and the healthy spawn height it
records does not match the current default robot (`g1_29dof_agile`, spawn
0.842). That doc predates the current default.

## What would settle it

The step-0 normal-contact impulse against the CUDA golden (`1.697e-1`), which
needs either `debug_contact_impulses()` ported on this branch or a CUDA host as
reference. The most direct next probe would be reading the mass matrix
immediately after `update_mprops` and again after the LU factorisation, to say
which of the two emits the first NaN.

## Reproduction

Add as `src/bin/dorobot_diag.rs` in zealot and run
`BIPED_SPAWN_DR=0 cargo run --release --bin dorobot_diag --features "gpu biped_gpu"`.

```rust
#[path = "../biped/biped_env_nexus.rs"]
mod biped_env_nexus;

use biped_env_nexus::{BipedNexusBatchEnv, default_mjcf_path};
use zealot_env::robots::NUM_JOINTS;

fn summarise(name: &str, v: &[f32]) {
    let nan = v.iter().filter(|x| x.is_nan()).count();
    let zero = v.iter().filter(|x| **x == 0.0).count();
    let finite_max = v.iter().filter(|x| x.is_finite()).fold(0.0f32, |a, b| a.max(b.abs()));
    println!("  {name:<12} len={:<7} nan={nan:<7} zero={zero:<7} max|finite|={finite_max:.6}", v.len());
}

fn main() {
    let xml = std::fs::read_to_string(default_mjcf_path()).expect("read mjcf");
    pollster::block_on(async {
        let mut env = BipedNexusBatchEnv::new(&xml, 1, 1, 0xC0FFEE).await;
        let _ = env.reset_env_to_default_template(0).await;
        env.pin_command_for(0, 0.0, 0.0, 0.0);

        println!("BEFORE step:  torso z = {:?}", env.torso_heights().await);
        let (dof, mm) = env.dbg_mb_dof_state_and_lu().await;
        summarise("dof_state", &dof);
        summarise("mass_matrix", &mm);

        let outs = env.step(&[[0.0f32; NUM_JOINTS]]).await;

        println!("AFTER one step: torso z = {:?}  done={} fell={}",
                 env.torso_heights().await, outs[0].done, outs[0].fell);
        let (dof, mm) = env.dbg_mb_dof_state_and_lu().await;
        summarise("dof_state", &dof);
        summarise("mass_matrix", &mm);
    });
}
```
