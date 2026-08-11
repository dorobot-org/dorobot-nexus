# dorobot-nexus

A native training studio for GPU reinforcement learning on humanoids, written in
Rust on [makepad](https://github.com/makepad/makepad).

The goal: put the simulator and the console in **one process**, so the viewport
is the simulation rather than a video of it — which is what makes
single-stepping, perturbing a running policy and zero-latency scrubbing possible
at all. Every other trainer in this space is a headless job plus a browser tab
and cannot offer them at any price.

It is a sibling to [DoRobot Studio](https://github.com/dorobot-org/dorobot-studio),
not a mode inside it. They share a visual language and two widget crates; they do
not share a codebase, because they have two operators. One runs a robot for
minutes with their hands on it. The other trains a policy for hours with their
hands off.

---

## Quick start

```bash
tools/fetch_robot_meshes.py g1     # 20 MB, once — meshes are fetched, not committed
cargo run --release                # the studio, with a live run in progress
```

That needs nothing but a Rust toolchain. It launches the built-in CPU learner,
which trains while you watch.

```bash
cargo run --release -- --headless 2000000   # train without a window
cargo run --release -- --sweep              # print the robustness surface as text
cargo run --release -- --track-check        # does the policy respond to its command?
```

### With the zealot GPU backend

```bash
./scripts/setup-zealot.sh                   # clones + builds the GPU stack (slow, once)
cargo run --release --features zealot       # same studio, GPU biped behind it
```

`setup-zealot.sh` clones seven upstream repos at pinned revisions, installs the
real `cargo-gpu`, primes the rust-gpu toolchain, applies one patch and builds
zealot's trainer. It is idempotent and it fails loudly rather than silently
producing something subtly wrong. Everything lands in `zealot-stack/`, which is
gitignored. Without it, `--features zealot` falls back to the CPU learner and
says so, rather than presenting a dead screen.

---

## The two backends

Both produce the same `Sample` stream, so the screens do not know which is
running. That seam is the whole architecture: `trainer.rs` promised "the trainer
is a job behind a metric stream, so it can be swapped without touching a
screen", and `zealot.rs` is that swap.

| | **CPU learner** (default) | **zealot GPU** (`--features zealot`) |
|---|---|---|
| Task | balance + velocity tracking, inverted pendulum | Unitree G1 biped locomotion |
| Physics | `src/env.rs`, vectorised, in-process | nexus on the GPU via khal → Metal/Vulkan/CUDA |
| Learner | `src/rl.rs` — PPO/GAE/Adam, hand-rolled MLP | zealot's GPU PPO |
| Throughput | ~40k env-steps/s, 256 envs | ~1.9k samples/s, 256 envs |
| Linked how | compiled in | **subprocess**, parsed from stdout |
| Dependencies added | — | **none** |

zealot is driven as a subprocess rather than linked, and that is forced rather
than stylistic — see [Building zealot](#building-zealot-and-why-it-looks-harder-than-it-is).

**Verified, CPU:**

```
$ dorobot-nexus --headless 1200000
     8192 steps  reward  0.636  falls 100.0%  ep_len    24      40k steps/s
   557056 steps  reward  0.610  falls 100.0%  ep_len    66      41k steps/s
   802816 steps  reward  0.713  falls   8.0%  ep_len   397      41k steps/s
  1171456 steps  reward  0.757  falls   0.0%  ep_len   400      41k steps/s
```

**Verified, zealot on Apple Silicon** — 600 iterations, 512 envs, flat ground:

```
iter 100  −0.4108   falls 5674   ← trough
iter 300  −0.3336   falls 5100
iter 500  −0.1523   falls 2763
iter 599  −0.0956   falls 2012   curriculum 0.00 → 0.40
```

Reward up 77% from the trough, falls down 65%, monotonic, while the curriculum
*raised* difficulty.

**But the policy is not good, and the reward curve hides it.** Mean episode
length from the same log:

| iter | fall rate | mean episode |
|---|---|---|
| 0 (random init) | 5.4% | **18.6 steps** |
| 100 | 46.2% | 2.2 steps |
| 599 | 16.6% | **6.0 steps** |

Zero episodes ever reached the time limit, and the trained policy survives
*less* long than the untrained one. Rising reward came from reward-term shaping,
not from staying upright. 600 iterations is also well short of zealot's own
reference (2000 iterations at 1024 envs, reward positive by ~250; this run is
still negative at 599).

So what is demonstrated is that **the training loop works on Metal** — finite
physics, live PPO, a reward that responds — not that a walking policy has been
produced. An earlier version of this file claimed "84% survival" for the trained
policy; that metric counted the fraction of frames that were not resets, which
flatters a robot falling every six steps. Corrected rather than deleted.

---

## Benchmarks

Apple Silicon, macOS 25.5, best-of-N per configuration. **Caveat:** the host was
not idle during measurement (other builds running); single samples varied 12k →
27k → 40k env-steps/s on the CPU before settling, so these are best-of and an
idle machine would do better.

### zealot GPU (Metal), G1 biped

| envs | s/iter | samples/iter | samples/s |
|---:|---:|---:|---:|
| 256 | 3.2 | 6,144 | 1,920 |
| 512 | 3.9 | 12,288 | 3,151 |
| 1024 | 5.6 | 24,576 | 4,388 |

4× the environments buys 2.3× throughput, so per-env cost keeps falling — the
whole argument for batching on GPU. zealot's own docs claim ~4.1 s/iter at 1024
envs on an M-series Mac against the 5.6 measured here; same ballpark on a busy
machine.

Per-step cost breakdown at 1024 envs (ms):

```
gpuwait=127.8   reward=20.6   commit=16.8   pipe=11.7   readback=4.6   flush=2.6
```

`gpuwait` dominates, so this is genuinely GPU-bound — the host is not the
bottleneck and a faster GPU would show up directly.

### CPU learner, cart-pole

| envs | env-steps/s |
|---:|---:|
| 256 | 38,000–40,000 |

### Why these two numbers must not be compared

The CPU figure is ~20× larger and that comparison is meaningless, because the
workloads differ by orders of magnitude:

| | CPU learner | zealot GPU |
|---|---|---|
| Bodies | 2 | 39 links |
| Multibody DOF | ~2 | 31 |
| Actuated joints | 1 | 12 |
| Obs / action dims | 5 / 1 | 265 / 12 |
| Contacts, terrain | none | yes |

A cart-pole is a 2×2 mass matrix; the G1 is a 31-DOF articulated multibody with
an LU factorisation, contact solving and terrain. The multibody solve alone is
roughly cubic in DOF — about 3,700× more arithmetic per step before contacts are
counted. Per unit of physics work the GPU is far ahead, but that cannot be
turned into an honest ratio here: there is no common workload, since zealot has
no CPU biped trainer and this repo has no GPU cart-pole.

---

## The six surfaces

| Surface | Question it answers | Fed by |
|---|---|---|
| **Runs** | What have I trained, and which one was actually good? | run model |
| **Scene** | What world and what body? | zealot rollout — the G1 animates from a real policy |
| **Task** | What does "good" mean, in terms that can be blamed individually? | metric stream |
| **Train** | Is it learning, and is it learning the right thing? | metric stream |
| **Inspect** | What is the policy actually doing, and what does it see? | CPU probe |
| **Validate** | Will this survive a real robot? | zealot sweep + sim-to-sim, or CPU |

The order is the order the operator's questions arrive in over the hours a run
takes. Full design rationale, including mockups, is in
[the UX spec](https://github.com/dorobot-org/dorobot-studio/blob/main/docs/nexus-ux/nexus-ux-design.html).

**Inspect stays on the CPU probe deliberately.** It draws a cart-pole, and
feeding G1 data into that drawing would misrepresent what is on screen.
Changing it means replacing the visual, not the data source.

---

## What is real

**The learners.** PPO/GAE/Adam with a hand-rolled MLP and explicit backward
passes (`src/rl.rs`, gradient-checked against finite differences); a vectorised
environment (`src/env.rs`); a seeded reproducible RNG (`src/rng.rs`); the
trainer on its own thread publishing snapshots over a channel. 44 tests, 46
with the zealot backend compiled in.

**Checkpoints carry their provenance.** Weights every 250k env-steps beside a
manifest with run, scene, seed, step, score, network shape and the reward terms
with their weights — so a blob on disk is always traceable to what produced it.

**Inspect drives them.** Loads the newest checkpoint, runs the policy
deterministically (its mean action, so stepping back and forth shows the same
trajectory twice), and gives you play, pause, single-step, scrub, restart and
**push**. The push is the one a headless trainer cannot offer: it applies an
impulse, discards the future it invalidated, and re-simulates from there. On a
trained policy you watch it recover — pole knocked to −0.15 rad, the policy
answering at 0.88 of full effort, reward dropping and climbing back. Restart
pulls in any newer checkpoint, so a probe left open during training does not
stay stuck on the policy it first loaded.

**Domain randomization.** Pole mass, actuator authority and cart drag are drawn
per episode from a declared distribution, and training runs across it.

**The robustness sweep.** Validate runs the newest checkpoint across a grid of
physics, several episodes per cell, scoring how well the policy did the task
there. On CPU that is force 0.25–2.0× and pole mass 0.3–5.0×, deliberately
wider than the training distribution. On zealot it is PD gain 0.4–1.6× and
ground friction 0.25–1.25, driven through zealot's own `BIPED_*` knobs. It runs
on its own thread and fills in live.

**Cross-simulator validation.** The same policy against two numerical
implementations of the same equations — semi-implicit Euler versus RK4 on CPU,
two physics decimations on zealot — with identical seeds, reporting the worst
relative gap. It is a weaker check than zealot's MuJoCo comparison and the UI
says so: sharing the dynamics function, it catches a policy that depends on
integration error but cannot catch a modelling error.

**Collapsed rollouts are refused, not scored.** See
[the reset trap](#the-trap-that-hid-it) below.

## What is not real

- **No robot import.** That control logs that it is unbuilt rather than
  pretending.
- **Scene's randomization sliders** display the distribution but cannot edit it.
- **Runs 2 and 3 are fixtures**, and the screen says so.
- **Velocity tracking is not solved** on the CPU task — see below, and the app
  says so in words rather than hiding it.

---

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

The useful outcome is that the failure is a named entry in the diagnosis
catalogue. A run that is upright, not falling, and flat on tracking reports
*Command ignored*, with the live numbers — so the next person does not spend an
hour finding it by hand.

---

## Building zealot, and why it looks harder than it is

An earlier version of this file claimed the dimforge GPU stack "cannot be built
outside its authors' machines" and that "none of those forks are public". Both
claims were wrong. Every fork is public and the stack builds here. The record is
corrected rather than deleted, because the reasons the first attempt failed are
the useful part.

Three things made a buildable stack look unbuildable, and two were misreadings:

1. **`path = "../vortx-unified"` names a directory, not a repo.** The `-unified`
   suffixes are *branches* — `vortx/unified`, `khal/unified` — on public forks.
2. **A `[patch]` whose version does not satisfy the requirement is silently
   ignored.** Cargo links the crates.io crate and says so only in a warning:
   `patch 'vortx v0.2.0' was not used in the crate graph`. This bites twice:
   parry's `spirv-compat` branch is 0.26.1 while nexus requires 0.29, so the
   correct branch is `rebase/nexus-0.4` — and the symptom of getting it wrong is
   stock parry3d under the GPU solver, which reads as a physics bug. Check
   `grep patch.unused Cargo.lock`; an unused patch is a broken build that still
   compiles.
3. **The crates.io crate named `cargo-gpu` is a placeholder** that prints
   "Coming Soon" and exits 0. khal's build script shells out to it, sees
   success, emits no SPIR-V, and the failure surfaces thousands of lines later
   as a runtime panic about a missing `.spv`. The real tool is
   `Rust-GPU/cargo-gpu`.

`setup-zealot.sh` encodes all of it and refuses to finish on a dropped patch.

### Why a subprocess, not a crate

zealot's `lib.rs` exposes nothing but `mod guides` — the training code lives in
`src/bin/` behind `#[path]` includes — and it path-depends on five sibling
checkouts outside its own repo. So it cannot be a git dependency, and cannot be
an optional path dependency either: cargo loads every dependency manifest during
resolution whether or not a feature enables it, so a missing sibling breaks a
clean clone. Reading its stdout keeps zealot's Rust untouched and keeps
`cargo build` here working with nothing but a toolchain.

Sharing one GPU device was never on the table anyway: makepad renders through
Metal directly on macOS (no wgpu in `makepad-platform`), so nexus and the UI
would be two GPU contexts in one process regardless. A process boundary costs
nothing that was available to begin with.

---

## The Metal bugs, and their fixes

zealot's sim used to die inside a single `env.step()` on Apple Silicon. Two
distinct miscompiles, both of the same class: naga's MSL backend drops the
**final** iteration of a `while` loop, because the `break_if` is re-evaluated
after the loop variables have already advanced.

**The loud one.** In nexus's fused FK/Jacobian/CRBA kernel:

```rust
let mut ti = 0u32;
while ti < sd.ndofs { …; ti += 1; }   // ndofs == 1 for a revolute joint
```

A one-iteration loop that ran **zero** times — leaving the DOF chain empty, the
Jacobian unwritten and the mass matrix never assembled. Factorising that
all-zero mass matrix produced NaN, and every pose, velocity and reward followed
one step later.

**The quiet one.** The integration loop in the same file:

```rust
let mut k = lane;
while k < num_links { integrate_link(…, k, …); k += t; }
```

This runs several iterations, so dropping the last one is not fatal — the last
link assigned to each lane simply never integrates, every step. No NaN, no
crash, just a quietly wrong simulation. Measured with the zero-action probe, 32
envs, fixed seed, mean first-episode length: **43.0 → 54.3 steps**, against
run-to-run noise of ±0.1. It survived an entire round of testing because
everything looked healthy.

Both are fixed by bounded, fully predicated loops — which is nexus's own
established remedy: its collision code already carries six comments reading
*"we use fixed-size for loops to avoid miscompilation issues of while loops on
MacOs"*. The authors knew this bug class; they just missed the multibody
kernels.

`patches/nexus-metal-jacobian-loop.patch`, applied by `setup-zealot.sh`.
Nothing is forked. Full measurements, the reproduction, and the three loops
deliberately *not* patched are in
[docs/zealot-metal-nan.md](docs/zealot-metal-nan.md).

### The trap that hid it

**zealot resets on termination before recording the next frame.** A policy that
falls on step one still produces a clean, finite, upright trajectory — the spawn
pose, over and over. A broken run gave 51 frames with 50 resets and looked
perfectly healthy.

The test that exposes it: run the same rollout with a trained policy and with
all-zero actions. Byte-identical output means the policy has no effect and every
frame is a reset. `Rollout` therefore parses `resets` and exposes `collapsed()`,
and the sweep reports such cells as unmeasured rather than scoring them — a grid
full of plausible numbers that measure nothing is worse than an empty one.

---

## Design rules

1. **The run is the document.** Every artifact hangs off a run. The checkpoint
   graveyard is the natural end state of every training tool that lacks this.
2. **Name every term.** A reward is named weighted terms, never an anonymous
   scalar — naming is what makes per-term curves and per-term blame possible.
   This earned its keep: a run whose total reward was *falling* had every
   behavioural term improving 2–6×, masked by a growing termination penalty. One
   curve would have read as failure.
3. **Diagnose, don't display.** Showing a curve is table stakes. The app matches
   known failure signatures and names them in words.
4. **Long work is abandonable.** Progress in env-steps, not spinners.
5. **Provenance travels.** A checkpoint carries its scene, weights and seed.
6. **One panel vocabulary.** Inherited from DoRobot Studio unchanged.
7. **Refuse to report what was not measured.** Unmeasured cells stay unmeasured;
   NaN reaches the plot rather than being swallowed.

---

## Layout

```
src/ux.rs          design tokens, chrome, nav rail
src/plot.rs        multi-series line plot widget
src/rl.rs          PPO, GAE, Adam, MLP with explicit backward (tested)
src/env.rs         vectorised balance-and-track environment (tested)
src/rng.rs         seeded reproducible RNG (tested)
src/trainer.rs     the training thread and its metric channel
src/zealot.rs      the zealot backend: subprocess, metric parsing, rollouts (tested)
src/ckpt.rs        checkpoint blobs + manifests (tested)
src/probe.rs       deterministic rollout with push/scrub (tested)
src/sweep.rs       robustness surface, CPU and zealot (tested)
src/crosssim.rs    sim-to-sim comparison, CPU and zealot (tested)
src/state.rs       run model + the diagnosis catalogue (tested)
src/screens/       the six surfaces
scripts/           setup-zealot.sh
patches/           the nexus Metal fix, applied not forked
docs/              the Metal root-cause write-up
data/g1/           Unitree G1 URDF (BSD-3-Clause, Unitree Robotics)
```

## Tests

```bash
cargo test                     # 44 tests
cargo test --features zealot   # 46 — adds the zealot parsers
```

The zealot parsers are tested against **verbatim** output captured from real
runs, not reconstructed fixtures — a parser for someone else's stdout is worth
exactly as much as its fidelity to the real thing.

## Troubleshooting

| Symptom | Cause |
|---|---|
| `--features zealot` trains on CPU | `zealot-stack/` not built — run `scripts/setup-zealot.sh` |
| Shader build "succeeds" but emits no `.spv` | crates.io `cargo-gpu` placeholder; install from `Rust-GPU/cargo-gpu` |
| Robot NaNs / falls through the floor | a `[[patch.unused]]` in zealot's `Cargo.lock`, or the nexus patch not applied |
| `Attempted to ask for consent when there's no TTY` | prime the toolchain: `cargo gpu install --shader-crate <path> --auto-install-rust-toolchain` |
| Rollout looks healthy but the policy does nothing | check `resets` — see [the trap](#the-trap-that-hid-it) |

## Credits

[zealot](https://github.com/haixuanTao/zealot) is the training stack this is a
console for, and `src/rl.rs` implements the same algorithm its `zealot-rl`
implements on the GPU (itself a port of rsl_rl);
[nexus](https://github.com/dimforge/nexus) is the physics engine underneath it.
The G1 model is from
[unitree_ros](https://github.com/unitreerobotics/unitree_ros), BSD-3-Clause.
