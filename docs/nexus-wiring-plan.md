# nexus-studio ↔ dorobot-nexus: wiring plan

> **Superseded in part.** This plan was written when the console was a separate
> repository driving the engine over a process boundary. Both consoles now live
> in this repo and **link** `nexus-engine` directly, so the problems that were
> really process-boundary problems — P2, P3, P5, P6 — dissolve rather than get
> fixed: there is no stream to parse, no child to kill, and no missing route.
> What survives is P1/P4/P7 and the reasoning, which is why the document is
> kept rather than deleted. Each item below is marked.

Status: **the engine half of items 3, 5 and 7 is implemented.** Everything
console-side is still proposed.
Written 2026-08-13 against `dorobot-studio` @ `5f3a252` (3 behind `origin/main`)
and `dorobot-nexus` @ `5c78875`.
Revised the same day: every claim re-verified against the tree, P1 corrected,
capability coverage and P6-P7 added, items 3 and 6 settled by the
canonical-backend rule below.
Implemented the same day on `dorobot-nexus` branch `headless-entry-points`:
`--capabilities`, `--crosssim`, `--probe`, and `--json` on every headless
surface. P2, P6 and P7 now have a backend; the console still has to consume it.
Also that day: item 1 landed, and `nexus-studio` was split out of
`dorobot-studio` into this repository, so the console now sits beside the engine
it drives rather than beside the LeRobot tooling it shares nothing with.

## What these two things are

They are two layers of one product, not two versions of it.

- **`dorobot-nexus`** (its own repo) is the engine: PPO/GAE/Adam with a
  hand-rolled MLP and explicit backward (`src/rl.rs`), a vectorised environment
  (`src/env.rs`), checkpoints with provenance (`src/ckpt.rs`), the robustness
  sweep (`src/sweep.rs`), sim-to-sim (`src/crosssim.rs`), the deterministic
  probe with push (`src/probe.rs`), and the zealot GPU backend behind a feature
  (`src/zealot.rs`). It ships six surfaces and a headless CLI.
- **`nexus-studio`** (in this repo) is the lifecycle console — the makepad port
  of the approved web mockup, eight workspaces including Home, Robots and
  Deploy. It has no learner. It drives the engine and reads its artifacts.

Everything that *computes* lives in the engine. Everything about the lifecycle
*around* a run — importing a robot, promoting a policy, certifying a deploy,
curating recordings — lives in the console, and much of it is mock by design
and labelled as such.

**`dorobot-nexus` is the canonical backend.** Where the two disagree the engine
is right, and a capability the console needs gets *added to the engine and
driven* rather than reimplemented on the console side. That is not a style
preference — it is what keeps a second learner from growing in a repo that
declares it has none. It settles items 3 and 6 below, and it is why P6's fix is
an engine change rather than a console workaround.

## How they are coupled today

| Coupling | Mechanism | Where |
|---|---|---|
| Schema | `dorobot-nexus` as a library, `default-features = false` (zero deps), `pub use dorobot_nexus::scene` | `nexus-studio/Cargo.toml`, `src/nexus.rs:13` — **on `origin/main` only** |
| Process | Spawns the real binary, streams stdout/stderr line by line over a channel | `src/nexus.rs:207`, `src/main.rs:835` and `:846` |
| Artifacts | Same files on disk, both directions | `real_ckpts()`, `hash_file()`, `scene::list()`, `load_rollout()`, `save_scene()`, `delete_scene()` |

Two actions launch real work: `real-sweep` runs `dorobot-nexus --sweep`
(`src/main.rs:833`), and `real-train` runs `dorobot-nexus --headless 2000000`
(`:845`), both with the working directory set to the engine repo.

## What the restructure changed

The console used to spawn `dorobot-nexus` and read its stdout. It now depends on
`nexus-engine` as a crate, in the same process, exactly as `nexus-app` always
has. That single change settles most of this plan:

| Problem | Was | Now |
|---|---|---|
| P2 run invisible while running | parse a stdout stream | read `Shared` off the trainer thread |
| P3 run cannot be stopped | needs `child.kill()` | `Handle::stop()`, already in `trainer.rs` |
| P5 Inspect asserts fake physics | two implementations | one `probe::Probe`, called directly |
| P6 probe/crosssim unreachable | no CLI existed | direct calls; the CLI is a bonus |
| P1 engine path hardcoded | **still real** | the console still resolves a *data* directory |
| P4 run not parameterised | **still real** | pre-flight still passes no options |
| P7 backend undetectable | **still real** | `zealot` is still a compile-time feature |

The headless CLI keeps its value — scripting, CI, and reproducing a run without
a window — but it is no longer the console's only route to the engine, and the
JSON contract is no longer load-bearing for the UI.

## Capability coverage

Engine entry points are dispatched in one place, `maybe_headless()` in
`dorobot-nexus/src/main.rs`. Every capability now has one — that was the work
described above — so the remaining gap is entirely in the third column:

| Engine capability | Entry point | Console uses it |
|---|---|---|
| training progress | `--headless N [--json]` | partly — spawns it, discards the output (P2) |
| robustness sweep | `--sweep [--json]` | yes, by scraping the text table |
| live probe | `--probe [run]` | no — Inspect still only toasts (P5) |
| sim-to-sim | `--crosssim [run]` | no — Validate has no data path |
| build & backend report | `--capabilities` | no — still infers from a filename (P7) |
| command tracking | `--track-check` | no |
| curriculum | `--curriculum <iters> <rounds>` | no, and zealot builds only |

`--json` is a modifier, not a mode: every surface above honours it and emits one
object per line. That is the contract the console should read. The fixed-width
text beside it exists for humans and is free to change — which is exactly why
the sweep row above, the one place the console *does* consume engine output, is
still the fragile one.

Home, Robots and Deploy have no engine counterpart and are not meant to; that is
console-side lifecycle, mock by design and labelled. The gap that matters is
Validate and Inspect, where the console presents an engine-shaped screen and now
has an engine available behind it.

## Problems, with evidence

**P1 — The engine path is hardcoded to one machine, in four places.**
`NEXUS_REPO` and `NEXUS_BIN` (`src/nexus.rs:15-17`) are absolute strings, and
they feed more than the spawn: `init_env()` sets `DOROBOT_SCENES_DIR` from them
(`:21`), `real_ckpts()` reads the repo directory (`:93`, `:98`), `spawn()` sets
`current_dir` (`:210`), and `actions.rs:1365` builds rollout paths. On another
clone this is not two dead buttons — scenes, checkpoints and recordings all
resolve into a directory that isn't there.

The failure is silent rather than loud. `bin_exists()` (`:203`) already gates
both real actions (`screens.rs:869`, `:937`), so on another machine they are
never offered — they do not fail at spawn. The gate is already built; what is
missing is the reason attached to it.

Upstream's PR #3 fixed the *schema* include this way but the *engine* path is
still literal — the same defect one layer down, and the same class as the old
`#[path = "/Users/…/scene.rs"]`.

**P2 — A real run is invisible while it runs.**
`drain_real_proc` (`src/main.rs:713`) parses sweep rows with `parse_sweep_row`
but keeps training output only as the last 200 raw lines. The engine already
prints a fixed-width progress line per interval — step, reward, fall rate,
episode length, steps/s — and none of it reaches the Train screen. So the
console can have a real run in flight while its charts show the mock stream,
distinguishable only by a toast.

**P3 — A launched run cannot be stopped.**
Nothing calls `kill()`. `real_proc` is cleared only when the child exits on its
own (`src/main.rs:732`). "Long work is abandonable" is design rule 4 in the
engine's own README; the console currently breaks it.

**P4 — The run is not parameterised.**
`real-train` is fixed at `--headless 2000000` on the CPU backend. The engine
also accepts `--no-random`, `--curriculum`, and (built with `--features zealot`)
a GPU path, none reachable from the console.

**P5 — Inspect's push button asserts physics that never ran.**
The engine's Inspect drives live physics with push-perturbation and
re-simulation (`probe.rs`); the console's replays recorded rollout frames into
the URDF view. That difference alone would be defensible — except that `"push"`
in `dispatch_named` (`src/main.rs`) does nothing but raise the toast *"Impulse
applied — future discarded, re-simulated from here"*. This is not two
implementations about to drift. It is one implementation and one claim with
nothing behind it, and it is the single mock here that isn't labelled as one.

**P6 — Probe and sim-to-sim have no route at any price.**
`crosssim` is constructed only from the engine's own event handlers
(`main.rs:682`, `:686`) and read by its Validate screen; `probe::Probe` the
same (`main.rs:600`, `:640`, `:652`). Neither has a CLI flag. The console
cannot reach them across the process boundary it uses — not because a call is
missing on the console side, but because the engine offers no entry point.
Validate's sim-to-sim story has no path forward that doesn't start in the
engine repo.

**P7 — The backend is undetectable.**
`zealot` is a compile-time Cargo feature (`dorobot-nexus/Cargo.toml`), so the
same binary name means different capabilities on different machines.
`bin_exists()` checks only that a file is at the path — one bit, and the wrong
one. The console cannot tell whether the engine it found has the GPU backend,
which is a precondition for offering it (item 5) or for `--curriculum`
(zealot-only).

## Plan

Ordered by value over effort. Each item states how to prove it works.

### 1. Take upstream's wiring (prerequisite) — done

The 3 commits are merged. The `#[path]` include is replaced by the
`dorobot-nexus` library dependency, and the local patch that repointed it at a
home directory was dropped rather than merged — it was only a machine-specific
path rewrite, which is the defect, not a fix for it.

Upstream also brought a `repo_dir()` resolver reading `DOROBOT_NEXUS_DIR`, which
is most of item 2 already: the remaining work there is routing the other three
call sites through it and stating a reason when it resolves to nothing.

*Verified:* `cargo check` clean; `grep -c '#\[path' nexus-studio/src/nexus.rs` is 0; no `/Users/` path remains anywhere in the crate.

### 2. Make the engine discoverable (P1, ~2 h)

Replace the two constants with a resolver, in this order: `DOROBOT_NEXUS_REPO`
env var → a sibling `../dorobot-nexus` next to this repo → `dorobot-nexus` on
`PATH`. Keep a single accessor so no call site learns the policy, and route all
four of P1's sites through it — scenes, checkpoints and rollout paths, not just
the spawn. Getting only the binary right leaves the console reading an empty
disk and saying nothing about why.

When nothing resolves, show the two real actions *disabled with the reason*
rather than omitting them, which is what happens now. The app already prefers
saying why over presenting a dead control; silence is worse than either.

*Verify:* fresh clone in a temp dir with the env var unset finds the sibling; with it set to a bogus path the buttons are visible, disabled and say so, and the scene and checkpoint lists read empty-with-reason rather than just empty; tests cover the resolver's three branches.

### 3. Parse the training stream (P2, ~3 h)

Teach the console to read the engine's progress lines into the same `live`
state the mock stream feeds, so Train shows the run it started. Parse defensively
— an unparsable line stays in the log rather than becoming a zero.

**Engine side: done.** `--headless N --json` emits one object per line —
`{"event":"start",…}`, one `{"event":"sample",…}` per interval, then
`{"event":"end",…}`. Reward terms are emitted **by name** rather than
positionally, so a reweighting cannot silently relabel a column; a non-finite
metric emits `null` rather than the literal `NaN` that would make the line
unparseable; and the poll now drains every interval published since the last
tick instead of only the newest, which it was quietly dropping.

**Console side: still to do.** Parse that stream into the same `live` state the
mock feeds. Parse defensively — an unparsable line stays in the log rather than
becoming a zero — and the parser gets tests against **verbatim captured
output**, the standard `zealot.rs` already sets in the engine.

*Verify:* start a real run, watch reward/falls/steps-per-second advance on Train; unit tests parse a captured run and reject a truncated line.

### 4. Stop button (P3, ~1 h)

Add `RealProc::stop()` (`child.kill()` then reap), wire it to the transport, and
report the outcome distinctly — cancelled is not finished. Also stop the child
on app exit so a closed window does not leave a trainer running.

*Verify:* launch, stop mid-run, confirm the process is gone (`pgrep`) and the toast says cancelled; relaunching afterwards works.

### 5. Parameterise the run (P4 and P7, ~3 h)

Surface env-steps, seed and randomisation in pre-flight, which already exists as
a dialog, and pass them through. `--headless N` and `--no-random` are accepted
by the engine today and need nothing from it.

The zealot option needed P7 solved first, and **that half is done**:
`--capabilities [--json]` reports the build's features, its available backends,
and the entry points it accepts, so a caller tests for a capability by name
rather than inferring it from a filename. It distinguishes *compiled in* from
*usable* — zealot is a subprocess, so the feature only says the client exists
and `zealot_binary` says whether the backend can actually run.

Console side: call it once per resolved binary, cache the answer, and offer the
GPU path and `--curriculum` only when it says so. Reading `entry_points` also
tells the console whether the engine it found speaks the JSON contract at all,
which is what makes items 3 and 7 safe to depend on.

*Verify:* a 100k-step run finishes in a fraction of the 2M default; `--no-random` reaches the engine (assert on the spawned argv in a test); against a CPU-only build the zealot option is absent *and the reason is shown*.

### 6. Make Inspect honest, then decide what it means (P5, ~1 h then design)

Do the honest part immediately and independently of the design: the `"push"`
handler must stop announcing a re-simulation it never ran. Remove the control
or mark it unavailable — a missing button is better than a false toast, and
this is a ten-minute change that should not wait behind an architecture
decision.

The design the canonical-backend rule constrains but does not settle: the
console does **not** grow its own probe. So either Inspect becomes a viewer for
recorded rollouts and live probing stays an engine screen, or the engine gains
a stepping mode the console drives — which is item 7's work. Pick one and write
it down before either side grows further.

*Verify:* no control in the console claims a simulation result it did not compute — grep the toast strings on the Inspect path and check each against something that actually ran.

### 7. Headless entry points for probe and sim-to-sim (P6) — engine side done

**`--crosssim [run]`** runs sim-to-sim on a run's newest checkpoint and prints
the comparison: both scores, the signed deltas, and `worst_gap`. It reports
`done` separately from "stopped running", because a weight/manifest mismatch
produces a report with no scores in it and emitting those zeros as a result
would be a lie. On a zealot build it prefers the decimation-against-decimation
comparison, mirroring what the GUI already chose.

**`--probe [run]`** drives a checkpoint one command at a time, reading
`step`/`seek`/`push`/`restart`/`state`/`quit` on stdin and reporting the
resulting state after each. `push` is the real thing — it truncates the
invalidated future and re-simulates, so the frame count visibly changes. A bad
command reports and the session continues; a driving process should not lose its
probe to a typo.

Console side: render `--crosssim` into Validate, and decide item 6 before
wiring `--probe` into Inspect.

*Verify:* done — `--crosssim --json` prints a parseable report against the checked-in `balance-track-01` checkpoints, and `printf 'step 40\nseek 0.5\npush 1.5\n' | dorobot-nexus --probe --json` shows the frame count drop as the future is discarded. The engine's own Validate and Inspect screens are untouched.

## Non-goals

- **Do not reimplement engine capability in the console.** The corollary of the
  canonical-backend rule: items 3, 5 and 7 all spend their effort in
  `dorobot-nexus` on purpose. A learner, a probe or a cross-sim that grows on
  the console side is a bug in this plan, not a shortcut through it.
- **Do not restyle `dorobot-nexus`.** Its violet-on-blue-black theme and its
  six-hue series palette stay as they are. An earlier attempt to unify them was
  reverted at the owner's request. This bars changes to how the engine *looks*,
  not the headless entry points items 3, 5 and 7 add — see the working-tree
  note below for how those are expected to land.
- **Do not develop the old single-screen `dorobot-studio` viewer.** Superseded.
- **`dorobot-flex` is out of scope here.**

## Known upstream defects we work around

Verified against makepad `dev` @ `d0fe5f2b` (2026-08-13) — none are fixed there,
so the workarounds stay ours to maintain.

1. **`TextInput` cannot be hidden.** It declares no `visible` property and
   `Widget::set_visible` has an empty default body, so hiding one compiles, runs
   and does nothing. Workaround: `nx.Field` in `crates/dorobot-ux` wraps it in a
   View, which does honour visibility.
2. **`empty_text` defaults to the literal "Your text here"**
   (`widgets/src/text_input.rs:43`), which ships as UI copy unless every site
   overrides it. Workaround: blanked in `nx.Field`; give each field a real hint.
3. **`sdf.box(…, 0.0)` has no interior.** With `r = 0` the distance field is
   zero everywhere inside, so fills and strokes silently vanish. Dev's recent
   change to `sdf.rs` only clamps *oversized* radii. Use **`sdf.rect(x,y,w,h)`**,
   which makepad already ships and whose field is correctly negative inside.

4. **`app_main!` writes to stdout before user code runs**, which makes a binary
   that has both a GUI and a CLI mode unable to emit a clean machine-readable
   stream. The generated `fn main` calls `Cx::init_log()` and then
   `init_websockets()`, and the latter logs `studio websocket disabled: empty
   studio_http` — to **stdout**, not stderr — before `AppMain::script_mod` is
   ever reached. Found while adding `--json`; verified against the vendored
   checkout `0cd882f`. Workaround: invoke `app_main!` inside a private `mod
   shell`, which demotes its `fn main` to `shell::main` where nothing calls it,
   and write the crate's real `fn main` to dispatch headless flags first. That
   also stops every headless invocation from starting a GUI stack it never uses
   — which on a machine with no window server is a failure, not an overhead.

Each is worth an upstream issue; (3) is a one-line fix using the formula
`rect` already uses, and (4) would be fixed upstream by logging to stderr.

## Repository state

Everything now lives in `dorobot-nexus`, branch `two-uis-one-engine`:
`crates/nexus-engine` (zero deps), `crates/nexus-app` (the six screens),
`crates/nexus-studio` (the eight workspaces, moved in with its history), and
`crates/makepad-plot`. 88 tests pass, 90 with the zealot feature; both binaries
build and both windows open.

The standalone `nexus-studio` repository that existed briefly between the two
restructures is now redundant — its content is here, with history. Delete it
once you are satisfied this layout is the one you want.

`dorobot-studio` keeps branch `lineage-split`, holding the `dorobot-ux`
extraction and the removal of the crates that moved out. Its `main` is untouched
at `5f3a252`.

**The one loose end is unchanged:** nothing is pushed, so `dorobot-ux` is still
a sibling path dependency rather than the git rev it should be.
