# dorobot-nexus

A native training studio for GPU reinforcement learning on humanoids, written in
Rust on [makepad](https://github.com/makepad/makepad).

The premise: [nexus](https://github.com/dimforge/nexus) renders physics on wgpu
and makepad renders on the GPU, so the simulator and the console can be **one
process on one device**. The viewport is then the simulation rather than a video
of it — which is what makes single-stepping, perturbing a running policy and
zero-latency scrubbing possible at all. Every other trainer in this space is a
headless job plus a browser tab and cannot offer them at any price.

It is a sibling to [DoRobot Studio](https://github.com/dorobot-org/dorobot-studio),
not a mode inside it. They share a visual language and two widget crates; they do
not share a codebase, because they have two operators. One runs a robot for
minutes with their hands on it. The other trains a policy for hours with their
hands off.

## Status

Six surfaces, zero script-VM errors, and **a trainer that actually learns**.

Launch it and the Train screen is showing a live run: PPO on 256 parallel
environments at ~41k env-steps/s, per-term reward curves, a contact sheet of
live poses with falls tinted, and the diagnosis rail reading real metrics.

The task is balance-and-velocity-tracking on an inverted pendulum — not the G1.
It is the smallest task carrying the *shape* of the whole-body problem (stay
upright while tracking a commanded velocity, with the same named reward terms),
so the loop, the reward decomposition and the diagnosis are exercised by real
numbers. Verified end to end:

```
$ dorobot-nexus --headless 1200000
     8192 steps  reward  0.636  falls 100.0%  ep_len    24      40k steps/s
   557056 steps  reward  0.610  falls 100.0%  ep_len    66      41k steps/s
   802816 steps  reward  0.713  falls   8.0%  ep_len   397      41k steps/s
  1171456 steps  reward  0.757  falls   0.0%  ep_len   400      41k steps/s
```

Falls 100% → 0%, episode length 24 → 400 (the cap). It solves the task.

**Real today:** PPO/GAE/Adam with a hand-rolled MLP and explicit backward passes
(`src/rl.rs`, gradient-checked against finite differences); a vectorised
environment (`src/env.rs`); a seeded reproducible RNG (`src/rng.rs`); the
trainer on its own thread publishing snapshots over a channel (`src/trainer.rs`);
the Unitree G1 at 29 DOF rendered from its real URDF; the diagnosis catalogue;
and a multi-series plot widget. 12 tests.

**Not real:** no GPU physics — see below. No robot import; that control logs
that it is unbuilt rather than pretending. Checkpoints are scored but not
written to disk. Runs 2 and 3 in the list are fixtures, and the screen says so.

## Why nexus and zealot are not linked

The design premise was that nexus and makepad could share one GPU device in one
process. Both halves of that turned out to be wrong here, and the evidence is
worth recording rather than quietly dropping.

**The dimforge GPU stack cannot be built outside its authors' machines.**
`nexus3d` is not published to crates.io — zealot depends on it by path, at
`../nexus/crates/nexus3d`. Its shaders need `cargo-gpu` (installable, and I did
install it), but the crates.io `vortx 0.3.0` then fails to emit its SPIR-V:

```
error: proc macro panicked
  --> vortx-0.3.0/src/lib.rs:12:38
   = help: ".../out/shaders-spirv" is not a directory
```

zealot's own manifest explains why: it redirects `vortx`, `khal` and `rapier3d`
to **unpublished local dimforge forks** via `[patch.crates-io]`, plus a vendored
`naga` carrying a fix for a Metal miscompilation — without which, in their
words, "the biped free-fell then launched on macOS". None of those forks are
public.

**And makepad does not use wgpu on macOS.** It renders through Metal directly
(`platform/src/os/apple/metal.rs`; no wgpu anywhere in `makepad-platform`). So
even with the stack building, nexus and the UI would be two GPU contexts in one
process on this platform — one process, but not one device.

The architecture the design already called for survives this intact: the trainer
is a job behind a metric stream. Swapping this CPU learner for a GPU one is a
change to `src/trainer.rs` and nothing else — no screen knows the difference.

## Running it

```
tools/fetch_robot_meshes.py g1     # 20 MB, once
cargo run --release                # the studio, training live
cargo run --release -- --headless 2000000   # train without a window
```

Meshes are fetched rather than committed. Upstream ships 167 files at 110 MB
covering every G1 variant; this URDF references 35 of them, and the script
derives that set from the URDF itself.

## The six surfaces

| Surface | Question it answers |
|---|---|
| **Runs** | What have I trained, and which one was actually good? |
| **Scene** | What world and what body? |
| **Task** | What does "good" mean, in terms that can be blamed individually? |
| **Train** | Is it learning, and is it learning the right thing? |
| **Inspect** | What is the policy actually doing, and what does it see? |
| **Validate** | Will this survive a real robot? |

The order is the order the operator's questions arrive in over the hours a run
takes. Full design rationale, including mockups, is in
[the UX spec](https://github.com/dorobot-org/dorobot-studio/blob/main/docs/nexus-ux/nexus-ux-design.html).

## Design rules

1. **The run is the document.** Every artifact hangs off a run. The checkpoint
   graveyard is the natural end state of every training tool that lacks this.
2. **Name every term.** A reward is named weighted terms, never an anonymous
   scalar — naming is what makes per-term curves and per-term blame possible.
3. **Diagnose, don't display.** Showing a curve is table stakes. The app matches
   known failure signatures and names them in words.
4. **Long work is abandonable.** Progress in env-steps, not spinners.
5. **Provenance travels.** A checkpoint carries its scene, weights and seed.
6. **One panel vocabulary.** Inherited from DoRobot Studio unchanged.

## Layout

```
src/ux.rs          design tokens, chrome, nav rail
src/plot.rs        multi-series line plot widget
src/rl.rs          PPO, GAE, Adam, MLP with explicit backward (tested)
src/env.rs         vectorised balance-and-track environment (tested)
src/rng.rs         seeded reproducible RNG (tested)
src/trainer.rs     the training thread and its metric channel
src/state.rs       run model + the diagnosis catalogue (tested)
src/screens/       the six surfaces
data/g1/           Unitree G1 URDF (BSD-3-Clause, Unitree Robotics)
```

## Credits

[zealot](https://github.com/haixuanTao/zealot) is the training stack this is a
console for, and `src/rl.rs` implements the same algorithm its `zealot-rl`
implements on the GPU (itself a port of rsl_rl);
[nexus](https://github.com/dimforge/nexus) is the physics engine underneath it.
Neither is linked, for the reasons above. The G1 model is from
[unitree_ros](https://github.com/unitreerobotics/unitree_ros), BSD-3-Clause.
