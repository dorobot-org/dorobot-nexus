#!/usr/bin/env bash
# Build zealot's GPU biped trainer for the --features zealot backend.
#
# zealot is not vendored and not forked: this clones it and the five sibling
# checkouts its manifest path-depends on, then builds it in its own workspace
# so its own [patch.crates-io] applies unchanged. Nothing here edits any of
# those repos.
#
# The directory names are load-bearing. zealot resolves "../nexus",
# "../khal-unified" and so on relative to itself, so the layout must be exactly
# what it expects, and cutile-rs must be present even on macOS (it is CUDA-only
# but cargo still parses its manifest).
#
# Everything lands in zealot-stack/, which is gitignored. Re-running is safe.
set -euo pipefail

cd "$(dirname "$0")/.."
STACK="$PWD/zealot-stack"
mkdir -p "$STACK"

# repo-url                                        dir             ref
#
# These are BRANCH names, and the checkout below takes each branch's tip. That
# does not reproduce a working stack: zealot's master runs ahead of its sibling
# branches, and building the tip fails with 17 errors — Ppo::stage_batch,
# vortx::linalg::PpoStageParams and RbdState::reset_envs_from_templates all
# missing, because master expects newer vortx/nexus than these tips provide.
#
# docs/train-on-macos.md pins exact revs that do build. Until this script pins
# them too, check out that table by hand after running it:
#
#   zealot 329fb3b · nexus d2451e0 · khal c6175be · vortx 1495fe4
#
# Verified 2026-08-14: at those revs biped_train_gpu builds on Apple Silicon.
PINS=(
  "https://github.com/haixuanTao/zealot.git       zealot          master"
  "https://github.com/haixuanTao/nexus.git        nexus           feat/browser-web-demo"
  "https://github.com/haixuanTao/khal.git         khal-unified    khal/unified"
  "https://github.com/haixuanTao/vortx.git        vortx-unified   vortx/unified"
  # NOT spirv-compat: that branch is parry 0.26.1 while nexus requires 0.29, so
  # the patch silently no-ops and stock parry3d gets linked under the GPU
  # solver — which reads as a physics bug, not a packaging one.
  "https://github.com/haixuanTao/parry.git        parry           rebase/nexus-0.4"
  "https://github.com/haixuanTao/naga-fixed.git   naga-fixed      vendored-20260805"
  "https://github.com/NVlabs/cutile-rs.git        cutile-rs       v0.2.0"
)

echo "==> checkouts"
for pin in "${PINS[@]}"; do
  read -r url dir ref <<<"$pin"
  if [ -d "$STACK/$dir/.git" ]; then
    git -C "$STACK/$dir" fetch -q origin "$ref" 2>/dev/null || git -C "$STACK/$dir" fetch -q origin
  else
    git clone -q "$url" "$STACK/$dir"
    git -C "$STACK/$dir" fetch -q origin "$ref" 2>/dev/null || true
  fi
  git -C "$STACK/$dir" checkout -q FETCH_HEAD 2>/dev/null || git -C "$STACK/$dir" checkout -q "$ref"
  printf '    %-14s %s @ %s\n' "$dir" "$ref" "$(git -C "$STACK/$dir" rev-parse --short HEAD)"
done

# The crates.io crate named cargo-gpu is a placeholder that prints "Coming
# Soon" and exits 0. khal's build script shells out to it, sees success, emits
# no SPIR-V, and the failure only surfaces much later as a runtime panic about
# a missing .spv. Detect that specifically rather than just checking presence.
# On Metal, one `while ti < sd.ndofs { … ti += 1 }` loop in nexus's fused
# FK/Jacobian/CRBA kernel runs ZERO times instead of once, leaving the DOF chain
# empty, the Jacobian unwritten and the mass matrix never assembled — its LU
# then produces NaN and the sim dies in a single step. Rewriting that loop as a
# bounded, fully predicated `for` fixes it. Applied here rather than forked, so
# the checkout stays upstream's. See docs/zealot-metal-nan.md.
echo "==> nexus Metal Jacobian-loop patch"
if git -C "$STACK/nexus" apply --check "$PWD/patches/nexus-metal-jacobian-loop.patch" 2>/dev/null; then
  git -C "$STACK/nexus" apply "$PWD/patches/nexus-metal-jacobian-loop.patch"
  echo "    applied"
elif git -C "$STACK/nexus" apply --reverse --check "$PWD/patches/nexus-metal-jacobian-loop.patch" 2>/dev/null; then
  echo "    already applied"
else
  echo "    WARNING: patch does not apply — upstream may have changed this kernel."
  echo "    Without it the sim NaNs on Metal; check docs/zealot-metal-nan.md."
fi

echo "==> cargo-gpu"
if ! command -v cargo-gpu >/dev/null 2>&1 || cargo gpu --version 2>&1 | grep -qi "coming soon"; then
  echo "    installing the real cargo-gpu from git"
  cargo install --git https://github.com/Rust-GPU/cargo-gpu cargo-gpu --locked
else
  echo "    $(cargo gpu --version 2>&1 | head -1)"
fi

# Build scripts have no TTY, and cargo-gpu asks for consent before installing
# its rust-gpu toolchain — khal hardcodes its `cargo gpu build` arguments, so
# the flag cannot be threaded through. Prime the cache here, where a TTY is not
# required because the flag can be passed directly.
echo "==> rust-gpu toolchain (first run compiles rustc_codegen_spirv; slow)"
for crate in "$STACK/nexus/crates/nexus_rbd_shaders3d" "$STACK/vortx-unified/vortx-shaders"; do
  cargo gpu install --shader-crate "$crate" --auto-install-rust-toolchain >/dev/null
  echo "    primed $(basename "$crate")"
done

echo "==> building biped_train_gpu (WebGPU/Metal on macOS, CUDA where present)"
cd "$STACK/zealot"
FEATURES="gpu biped_gpu"
if command -v nvidia-smi >/dev/null 2>&1; then FEATURES="$FEATURES cutile"; fi
cargo build --release --bin biped_train_gpu --features "$FEATURES"

# A dropped patch still compiles — it just links the wrong crate. This is the
# check that would have caught stock parry3d being used under the GPU solver.
if grep -q "patch.unused" Cargo.lock; then
  echo
  echo "WARNING: Cargo.lock contains unused patches — a fork is being silently"
  echo "ignored because its version does not satisfy the requirement:"
  grep -A2 "patch.unused" Cargo.lock | sed 's/^/    /'
  exit 1
fi

echo
echo "built: $STACK/zealot/target/release/biped_train_gpu"
echo "run:   cargo run --release --features zealot"
