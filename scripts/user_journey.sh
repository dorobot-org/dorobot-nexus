#!/bin/zsh
# Drive nexus-studio exactly as a user would — through its own soak-driver
# seam (the same actions pointer clicks dispatch) — and capture the window
# after each step. Run from the repo root while the studio is up.
set -e
CMD=/tmp/nexus_cmd.txt
STATE=/tmp/nexus_state.json
OUT=runs/rough-slope-8deg/journey
mkdir -p "$OUT"
WID=$(swift /tmp/get_window_id.swift nexus-studio)

ack() { python3 -c "import json,sys; print(json.load(open('$STATE')).get('ack',0))" 2>/dev/null || echo 0; }

step() { # step <name> <settle_s> <commands…>
  local name=$1 settle=$2; shift 2
  local before=$(ack)
  for c in "$@"; do echo "$c" >> "$CMD"; done
  for i in {1..100}; do
    [ "$(ack)" != "$before" ] && break
    sleep 0.2
  done
  sleep "$settle"
  screencapture -x -o -l "$WID" "$OUT/$name.png"
  echo "step $name done (ack $(ack))"
}

# 1 · Scenes: define the rough-slope scene with randomness, save to disk.
step 01-scenes 1 "mode scenes"
step 02-new-scene 1 "act new-scene"
step 03-terrain-rough 1 "arg fam:rough"
step 04-slope 0.5 "slider slope 8"
step 05-amp 0.5 "slider amp 1.0"
step 06-push 0.5 "slider push 0.5"
step 07-mass 0.5 "slider mass 1.1"
step 08-save 1 "act save"

# 2 · Train: the live attached run — real curves.
step 09-train 2 "mode train"

# 3 · Validate: MuJoCo sim-to-sim on the same terrain.
step 10-validate 1 "mode validate"
step 11-mujoco-run 2 "act mujoco-run"
echo "waiting for the MuJoCo arm…"
for i in {1..240}; do
  # The replay only appears once the harness delivered its motion; use it
  # as the completion signal (dump_state exposes it as "replay": true).
  echo "mode validate" >> "$CMD"   # idempotent nudge so state re-dumps
  if grep -aq '"replay": true' "$STATE" 2>/dev/null; then break; fi
  sleep 5
done
sleep 2
screencapture -x -o -l "$WID" "$OUT/12-mujoco-verdict.png"
step 13-replay 3 "play"
screencapture -x -o -l "$WID" "$OUT/14-replay.png"
echo "journey complete → $OUT"
