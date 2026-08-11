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

The six surfaces are built and the app runs with zero script-VM errors. **The
trainer is not attached.** Metrics are generated, and the app says so on screen
rather than hiding it — a plausible curve that nothing produced is the most
expensive lie a tool like this can tell.

What is real today:

- **The robot.** The Unitree G1 at 29 DOF, rendered from its actual URDF through
  `makepad-urdf-player` — 39 links, 35 with meshes, 29 movable joints.
- **The diagnosis catalogue.** Named failure signatures matched against run
  metrics, in `src/state.rs`, with tests. Given a run whose reward climbs while
  its fall rate climbs, the app reports *reward hacking*, names the term
  climbing fastest, and says what to try.
- **The plot widget.** Multi-series curves resampled per pixel column, in
  `src/plot.rs`.
- **The screens.** Runs, Scene, Task, Train, Inspect, Validate — navigable, with
  real layout and real state flowing through them.

What is not:

- No simulator. `nexus` is not linked; the environment grid is schematic.
- No trainer. `zealot-rl` is not linked; there is no learning.
- No robot import. The Add robot flow is designed but not built, and the control
  logs that rather than pretending.

## Running it

```
tools/fetch_robot_meshes.py g1     # 20 MB, once
cargo run --release
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
src/state.rs       run model + the diagnosis catalogue (tested)
src/screens/       the six surfaces
data/g1/           Unitree G1 URDF (BSD-3-Clause, Unitree Robotics)
```

## Credits

[zealot](https://github.com/haixuanTao/zealot) is the training stack this is a
console for; [nexus](https://github.com/dimforge/nexus) is the physics engine
underneath it. Neither is linked yet. The G1 model is from
[unitree_ros](https://github.com/unitreerobotics/unitree_ros), BSD-3-Clause.
