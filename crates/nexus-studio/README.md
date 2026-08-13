# nexus-studio

The lifecycle console for `dorobot-nexus`. This repository is the workspace;
the application lives in [`nexus-studio/`](nexus-studio/), which has its own
README describing the screens.

## Where this sits

Three repositories, checked out as siblings:

    ~/home/dorobot/
    ├── dorobot-nexus/    the engine — PPO/GAE, the sweep, sim-to-sim, the probe
    ├── nexus-studio/     this repo — the console that drives it
    └── dorobot-studio/   the teleop and dataset tools, and dorobot-ux

`dorobot-nexus` is the **canonical backend**. Everything that computes lives
there; everything about the lifecycle *around* a run — importing a robot,
promoting a policy, certifying a deploy, curating recordings — lives here. When
this console needs a capability the engine has, the capability is added to the
engine and driven from here, never reimplemented on this side. The engine
exposes a headless entry point per capability (`--headless`, `--sweep`,
`--crosssim`, `--probe`, `--capabilities`), each speaking one JSON object per
line under `--json`.

`dorobot-studio` is a **separate product line** — LeRobot datasets, dora
dataflows, teleop. It shares exactly one thing with this repository: the
`dorobot-ux` visual language crate. That crate is kept lineage-neutral, so
nothing engine-specific belongs in it and nothing dataset-specific does either.

## Building

    cargo run -p nexus-studio

`dorobot-ux` currently resolves through a sibling path dependency, so
`dorobot-studio` must be checked out beside this repository. Once that crate is
pushed, the path in the workspace `Cargo.toml` becomes a git rev and this repo
builds from a bare clone — see the comment there.

The engine is found at runtime, not build time: set `DOROBOT_NEXUS_DIR`, or
leave it unset and the sibling checkout above is used.

## History

Split out of `dorobot-studio` in August 2026, where this console shared a cargo
workspace with the LeRobot tooling. The per-file history of both `nexus-studio/`
and `crates/makepad-plot/` came across with the split, so `git log` and
`git blame` reach back to the original port.
