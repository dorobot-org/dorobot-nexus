# nexus-studio

Makepad port of the approved dorobot-nexus web mockup (frozen spec:
claude.ai artifact d2f94801). Eight workspaces — Home, Scenes, Robots, Train,
Inspect, Validate, Runs, Deploy — factory-industrial theme, EN/中文 at runtime,
light/dark/auto theme, mock data and simulated evidence exactly as the spec
labels them.

Run:

    cargo run -p nexus-studio

- `src/state.rs` + `src/actions.rs` — the entire mockup state machine in pure
  Rust (pair-keyed deploy certification, generation tokens, health-gated dwell,
  checkpoint retention, undoable deletion) with 14 unit tests.
- `src/i18n.rs` — 523-entry EN→ZH dictionary generated from the mockup.
- `src/tokens.rs` — factory palette + Roboto / Noto Sans SC / Roboto Mono.
- `src/kit.rs` — custom-drawn Spark, Segsbar, Heat, Scrub widgets.
- `src/screens*` — per-mode rails, stage, timeline, home, modals, toasts.

The workspace also vendors `crates/makepad-plot` (from Splash-Makepad),
proven to compile against the same makepad dev rev for future real-data
charts. The 3D upgrade path replaces the stage's shader robot with
makepad-urdf-player's RobotView, already a dependency of dorobot-flex.
