# dorobot-nexus

A native training studio for GPU reinforcement learning on humanoids, written in
Rust on [makepad](https://github.com/makepad/makepad).

The goal: put the simulator and the console in **one process**, so the viewport
is the simulation rather than a video of it — which is what would make
single-stepping, perturbing a running policy and zero-latency scrubbing possible
at all. Every other trainer in this space is a headless job plus a browser tab
and cannot offer them at any price.

The original premise was stronger — one process *and one GPU device*, since
[nexus](https://github.com/dimforge/nexus) renders physics on wgpu. That part is
wrong on macOS, where makepad renders through Metal directly and does not depend
on wgpu at all. The trainer running here is in-process on the CPU; see
[why nexus and zealot are not linked](#why-nexus-and-zealot-are-not-linked).

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

**Checkpoints are written and reloadable.** The trainer writes weights every
250k env-steps beside a manifest carrying run, scene, seed, step, score, network
shape and the reward terms with their weights — so a blob on disk is always
traceable to what produced it.

**Inspect drives them.** It loads the newest checkpoint, runs the policy
deterministically (its mean action, so stepping back and forth shows the same
trajectory twice), and gives you play, pause, single-step, scrub, restart and
**push**. The push is the one a headless trainer cannot offer: it applies an
impulse, discards the future it invalidated, and re-simulates from there. On a
trained policy you watch it recover — pole knocked to -0.15 rad, the policy
answering at 0.88 of full effort, reward dropping and climbing back.

Restart also pulls in any newer checkpoint, so a probe left open while training
continues does not stay stuck on the policy it first loaded.

**Domain randomization is real.** Pole mass, actuator authority and cart drag
are drawn per episode from a declared distribution, and training runs across it.

**The robustness sweep is real.** Validate runs the newest checkpoint across a
grid of physics — force 0.25–2.0×, pole mass 0.3–5.0×, deliberately wider than
the training distribution — twelve episodes per cell, and scores each cell by
how well the policy did the task there. It runs on its own thread and fills in
live. `--sweep` prints the same surface as text, so two policies can be diffed.

**Cross-simulator validation is real.** The same policy runs against two
numerical implementations of the same equations — the semi-implicit Euler step
training uses, and a fourth-order Runge-Kutta reference — with identical seeds
and command spreads, and the panel reports the worst relative gap. It is a
weaker check than zealot's MuJoCo comparison and the UI says so: sharing the
dynamics function, it catches a policy that depends on integration error but
cannot catch a modelling error. The table is labelled `euler` / `rk4` rather
than borrowing a simulator's name this build does not have.

**Not real:** no GPU physics — see below. No robot import; that control logs
that it is unbuilt rather than pretending. Scene's randomization sliders display
the distribution but cannot yet edit it. Runs 2 and 3 are fixtures, and the
screen says so.

## What the sweep found

Worth recording, because it is the product working as intended and it is not a
flattering result.

The first sweep scored cells by *survival* and returned 100% everywhere — for a
policy trained with randomization **and** for a control trained without it. A
measurement that cannot fail is not a measurement. Scoring the task instead
(tracking quality, with a fall scoring zero) made the surface honest, and it now
reads about 0.40 uniformly.

That number is diagnostic. `--track-check` shows why: commanded −0.75 m/s, the
policy achieves −0.005. **It learned to balance and ignore the velocity
command.** 0.40 is exactly the mean of `exp(-3|cmd|)` over the command spread —
the score of standing perfectly still.

Chasing that turned up a genuine specification bug. Episodes terminated when the
cart left a ±3 m box, and that termination counted as a fall — but tracking any
command above 0.375 m/s leaves a 3 m box before the episode ends. The task
contradicted itself, and the policy correctly learned that moving meant death.
The track is now 30 m and running off it is no longer a fall.

Fixing it was not sufficient: the policy still stands still. Accelerating a
cart-pole forward requires first pushing backward to tip the pole, and that
non-minimum-phase manoeuvre is hard to discover. Wider reward kernels and more
entropy did not find it either, and both changes were reverted rather than left
in as unjustified tuning. **Velocity tracking is not solved here.**

The useful outcome is that the failure is now a named entry in the diagnosis
catalogue. A run that is upright, not falling, and flat on tracking reports
*Command ignored*, with the live numbers — so the next person does not spend an
hour finding it by hand.

## How zealot is linked

An earlier version of this file claimed the dimforge GPU stack "cannot be built
outside its authors' machines" and that "none of those forks are public". Both
claims were wrong. Every fork is public, and the stack builds here: 122 SPIR-V
modules compile, khal selects Metal/WebGPU, and 256 GPU environments step. The
record is corrected rather than deleted, because the reasons the first attempt
failed are the useful part.

Three things made a buildable stack look unbuildable, and two were misreadings:

1. **`path = "../vortx-unified"` names a directory, not a repo.** The `-unified`
   suffixes are *branches* — `vortx/unified`, `khal/unified` — on public forks.
2. **A `[patch]` whose version does not satisfy the requirement is silently
   ignored.** Cargo links the crates.io crate and says so only in a warning:
   `patch 'vortx v0.2.0' was not used in the crate graph`. Fork default branches
   are older versions, so patching without naming the branch quietly no-ops and
   the resulting type errors point anywhere but at the cause. This one bites
   twice: parry's `spirv-compat` branch is 0.26.1 while nexus requires 0.29, so
   the fix is `rebase/nexus-0.4`, and the symptom of getting it wrong is stock
   parry3d under the GPU solver — which reads as a physics bug.
3. **The crates.io crate named `cargo-gpu` is a placeholder** that prints
   "Coming Soon" and exits 0. khal's build script shells out to it, sees
   success, emits no SPIR-V, and the failure surfaces thousands of lines later
   as a runtime panic about a missing `.spv`. The real tool is
   `Rust-GPU/cargo-gpu`.

`scripts/setup-zealot.sh` encodes all of it, and fails loudly on a dropped
patch instead of building something subtly wrong.

**zealot is driven as a subprocess, not linked as a crate.** That is forced,
not stylistic. Its `lib.rs` exposes nothing but `mod guides` — the training code
lives in `src/bin/` behind `#[path]` includes — and it path-depends on five
sibling checkouts outside its own repo, so it cannot be a git dependency, and
cannot be an optional path dependency either: cargo loads every dependency
manifest during resolution whether or not a feature enables it, so a missing
sibling breaks a clean clone. Reading its stdout keeps zealot's Rust untouched
and keeps `cargo build` here working with nothing but a toolchain.

**Sharing one GPU device was never on the table anyway.** makepad renders
through Metal directly on macOS (`platform/src/os/apple/metal.rs`; no wgpu in
`makepad-platform`), so nexus and the UI would be two GPU contexts in one
process regardless — one process, but not one device. A process boundary costs
nothing that was available to begin with.

The architecture the design called for survives intact: the trainer is a job
behind a metric stream, so `src/zealot.rs` swaps the producer and no screen
knows the difference.

### The Metal NaN, and its fix

zealot's sim used to die inside a single `env.step()` on Apple Silicon: every
pose, velocity and reward went NaN, so nothing could learn. The cause was one
loop in nexus's fused FK/Jacobian/CRBA kernel —

```rust
let mut ti = 0u32;
while ti < sd.ndofs { ...; ti += 1; }   // ndofs == 1 for a revolute joint
```

— running **zero times instead of once** on Metal, which left the DOF chain
empty, the Jacobian unwritten and the mass matrix never assembled. Factorising
that all-zero mass matrix produced the NaN. Rewriting it as a bounded, fully
predicated `for` fixes it; `patches/nexus-metal-jacobian-loop.patch` is applied
by `scripts/setup-zealot.sh`, so nothing is forked.

GPU training on Metal works after that: finite rewards, real fall counts, live
KL, ~3.5 s/iteration at 256 envs, and the zero-action probe falls at ~44 steps
against the ~40 zealot documents. Full measurements and the reproduction are in
[docs/zealot-metal-nan.md](docs/zealot-metal-nan.md).

The trap worth knowing before trusting any rollout: **zealot resets on
termination before recording the next frame**, so a policy that falls on step
one still produces a clean, finite, upright trajectory — the spawn pose over
and over. A broken run here gave 51 frames with 50 resets and looked perfectly
healthy; running it with a trained policy and with all-zero actions produced
byte-identical output, which is what gave it away. `Rollout` therefore parses
`resets` and exposes `collapsed()`, and the sweep reports such cells as
unmeasured rather than scoring them.

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
