#!/usr/bin/env bash
# Build the MuJoCo sim-to-sim arm for `--mujoco` and Validate's MuJoCo button.
#
# crosssim's other two arms compare Euler against RK4, and one control
# decimation against another. Both share the dynamics function, so both catch
# integration artefacts and nothing else. MuJoCo brings its own contact model
# and solver, so it is the first arm that can catch a *modelling* error — the
# check that actually predicts transfer, and the one Unitree performs before
# sim-to-real.
#
# Nothing is vendored and nothing is forked. This clones what the rollout needs
# and builds a virtualenv beside it; the engine keeps zero dependencies because
# the whole arm is a subprocess, exactly as zealot is.
#
# The rollout itself is zealot's `scripts/sim2sim_g1_mujoco.py`. That is
# deliberate: it already encodes the observation frame, and that frame has
# conventions no one would re-derive correctly — last_action is lag-2,
# joint_vel is a finite difference, the PD target is clamp(default +
# 0.5*action) driven as an explicit torque at 200 Hz with the model's own
# actuators disabled, the frame is stacked 5 deep and Welford-normalised, and
# the gait clock is max(0, t-1)*dt/0.7. Get one wrong and the policy falls over
# for reasons that are export drift rather than physics, which reads as a
# transfer failure and is not one.
#
# Everything lands in mujoco-stack/, which is gitignored. Re-running is safe.
set -euo pipefail

cd "$(dirname "$0")/.."
STACK="$PWD/mujoco-stack"
mkdir -p "$STACK"

# --- interpreter -------------------------------------------------------------
# 3.10+ is required: MuJoCo publishes no wheel for 3.9, so pip there tries to
# build from source and stops on a missing MUJOCO_PATH. A virtualenv rather
# than whatever `python3` resolves to, so the arm does not depend on the
# machine's default interpreter being the right one.
PY=""
for c in python3.13 python3.12 python3.11 python3.10; do
  command -v "$c" >/dev/null 2>&1 && { PY="$c"; break; }
done
if [ -z "$PY" ]; then
  echo "need python 3.10+ for a MuJoCo wheel; found none" >&2
  exit 1
fi
echo "==> venv ($PY)"
[ -d "$STACK/venv" ] || "$PY" -m venv "$STACK/venv"
"$STACK/venv/bin/pip" install -q --disable-pip-version-check mujoco safetensors numpy
printf '    mujoco %s\n' "$("$STACK/venv/bin/python3" -c 'import mujoco; print(mujoco.__version__)')"

# --- scene -------------------------------------------------------------------
# The harness defaults to mujoco_playground's G1 flat-terrain, feet-only scene.
# Cloned rather than pip-installed because mujoco_playground is not on PyPI.
echo "==> playground"
if [ -d "$STACK/playground/.git" ]; then
  git -C "$STACK/playground" fetch -q origin || true
else
  git clone -q --depth 1 https://github.com/google-deepmind/mujoco_playground.git "$STACK/playground"
fi

# The scene references menagerie meshes by relative path, and playground
# vendors menagerie as a submodule a shallow clone does not fetch. Sparse: only
# unitree_g1 is wanted, and the whole repo is large.
echo "==> menagerie (unitree_g1 only)"
MEN="$STACK/playground/mujoco_menagerie"
if [ ! -d "$MEN/.git" ]; then
  git clone -q --depth 1 --filter=blob:none --sparse \
    https://github.com/google-deepmind/mujoco_menagerie.git "$MEN"
  git -C "$MEN" sparse-checkout set unitree_g1
fi
printf '    %s mesh files\n' "$(ls "$MEN/unitree_g1/assets" 2>/dev/null | wc -l | tr -d ' ')"

# --- harness -----------------------------------------------------------------
# From the zealot checkout, so run scripts/setup-zealot.sh first if it is absent.
H="$PWD/zealot-stack/zealot/scripts/sim2sim_g1_mujoco.py"
if [ ! -f "$H" ]; then
  echo "no harness at $H — run scripts/setup-zealot.sh first" >&2
  exit 1
fi

# One patch, applied to the checkout rather than committed into it: the harness
# opens ffmpeg *before* its rollout loop, so a machine with MuJoCo but no
# ffmpeg measures nothing at all. S2S_NO_VIDEO=1 runs it for its numbers alone,
# which is all this path wants. Idempotent — skipped when already applied.
echo "==> patch"
P="$PWD/patches/zealot-sim2sim-no-video.patch"
if git -C "$PWD/zealot-stack/zealot" apply --check "$P" 2>/dev/null; then
  git -C "$PWD/zealot-stack/zealot" apply "$P"
  echo "    applied zealot-sim2sim-no-video"
else
  echo "    already applied (or does not apply cleanly — check by hand)"
fi

echo
echo "done. Verify with:"
echo "  cargo run --release -p nexus-app -- --mujoco <ckpt> --cmd 0.3 --seconds 45"
