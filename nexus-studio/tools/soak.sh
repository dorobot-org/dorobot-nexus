#!/bin/zsh
# UI soak harness: send driver commands, wait for ack, capture the window.
SHOTS=${1:-/tmp/nexus_shots}
mkdir -p "$SHOTS"
WID=$(python3 -c "
import Quartz
wl=Quartz.CGWindowListCopyWindowInfo(Quartz.kCGWindowListOptionOnScreenOnly, Quartz.kCGNullWindowID)
c=[w for w in wl if str(w.get('kCGWindowOwnerName',''))=='nexus-studio']
print(c[0]['kCGWindowNumber'] if c else '')")
[ -z "$WID" ] && { echo "no window"; exit 1; }
# App Nap throttles background timers to ~1Hz — foreground the app so the
# 120ms drive/gate cadence is real during the soak.
python3 -c "
from AppKit import NSRunningApplication
import subprocess
pid=int(subprocess.check_output(['pgrep','-x','nexus-studio']).split()[0])
NSRunningApplication.runningApplicationWithProcessIdentifier_(pid).activateWithOptions_(2)" 2>/dev/null
sleep 0.5
touch /tmp/nexus_soak_on
trap 'rm -f /tmp/nexus_soak_on' EXIT
ACK=$(python3 -c "import json;print(json.load(open('/tmp/nexus_state.json'))['ack'])" 2>/dev/null || echo 0)
step() {
  local name=$1; shift
  for c in "$@"; do echo "$c" >> /tmp/nexus_cmd.txt; done
  local NEW=$ACK
  for i in {1..40}; do
    sleep 0.15
    NEW=$(python3 -c "import json;print(json.load(open('/tmp/nexus_state.json'))['ack'])" 2>/dev/null || echo "$ACK")
    [ "$NEW" != "$ACK" ] && break
  done
  ACK=$NEW
  sleep 0.35
  screencapture -x -l$WID "$SHOTS/$name.png"
  echo "step $name → ack $ACK  $(python3 -c "import json;d=json.load(open('/tmp/nexus_state.json'));print(d['mode'],d['modal'],'toast:',d['last_toast'][:60])" 2>/dev/null)"
}
