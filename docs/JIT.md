# Makepad JIT and iOS appearance

Nexus uses OctoSense's iOS light/dark sheets on desktop and runs through the
Relax Rust JIT. This does not package an iPhone app.

The integration follows [OctoSense main 388e2f38](https://github.com/OctoSense-org/OctoSense/commit/388e2f3882c1d7b3776fee2dc6b4d875abecc557)
and its [JIT branch 14f258d4](https://github.com/OctoSense-org/OctoSense/commit/14f258d409b3a2b394852a8fa23e185717fdc99e),
checked November 13, 2026. `jit-runtime.json` and Cargo pin both the framework
and runner to [Makepad 44d8220f](https://github.com/OctoSense-org/makepad/commit/44d8220f12acfa46157019267e119e0866dd6b05).
Use the corresponding iOS/JIT revision of the sibling `dorobot-studio` repository:
Nexus consumes its `crates/dorobot-ux` crate.

```sh
./tools/run_jit.sh --setup
./tools/run_jit.sh --check
./tools/run_jit.sh
./tools/run_jit.sh --nano
./tools/run_jit.sh --robot /path/to/model.urdf
```

`--nano` imports the bundled model through the normal portable robot importer
and opens Robots. Each explicit import creates a new library entry; later
launches can omit `--nano` and open the saved entry. Model import does not
configure or launch training. The toolbar appearance control cycles automatic,
light and dark modes.

The first launch builds the pinned runner in
`$XDG_CACHE_HOME/dorobot/makepad-jit/<revision>` (default `~/.cache/...`), shared
with DoRobot Studio. Install a current stable Rust toolchain, Git, Python 3
and a platform C compiler; Linux also needs Makepad's graphics and file-dialog
development libraries. `MAKEPAD_JIT_SOURCE` can reuse an exact pinned checkout;
`DOROBOT_JIT_RUNNER` can select a matching existing executable. The runner is
built with Cargo; the Studio itself is compiled from source by Relax.

Relax watches source edits and patches supported changes into the running
app. An edit to an active long-running function may require a restart. This
beta runner has its own resolver and does not accept Cargo's `--locked` flag;
the committed lockfile is used for native builds and validation.

Validated on Apple Silicon/macOS: JIT type/borrow check with zero errors, a
real Metal window, Nano import and rendering with 27 links, 26 joints, 17
movable joints and 134 mesh references. Native
`cargo test --locked --workspace --all-features` passes 173 tests; two optional
integration tests are ignored. GPU training, MuJoCo installation, deployment
and other platforms have not been newly validated under JIT. Existing setup
requirements still apply, and native Cargo builds remain available.

Workspace libraries, training artifacts, logs and JIT caches stay local and
are excluded from publication.
