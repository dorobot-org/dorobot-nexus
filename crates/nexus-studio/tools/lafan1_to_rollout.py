#!/usr/bin/env python3
"""Turn a LAFAN1-retargeted G1 motion into a rollout this studio can replay.

The viewer's built-in animation sweeps every joint on an independent sine — it
proves the kinematic chain moves, but it is not a gait, and it reads as the
robot wiggling. This writes real motion into the same rollout format a zealot
rollout produces, so the existing replay path drives it and no new playback
code exists to disagree with the real one.

    tools/lafan1_to_rollout.py --clip walk1_subject1 --seconds 20

Source: https://huggingface.co/datasets/lvhaidong/LAFAN1_Retargeting_Dataset
LAFAN1 mocap retargeted to the Unitree G1 29-DoF by numerical IK. The CSV is
36 columns at 30 fps: 7 floating base (x y z qx qy qz qw) then 29 joint angles.

Licensing, because it decides where the output may go: the retargeting work is
BSD-3-Clause (Unitree Robotics), but the underlying LAFAN1 mocap is Ubisoft's
under CC BY-NC-ND 4.0 — non-commercial, no derivatives. Generated recordings
are gitignored user data (`scenes/`) and are fine as a local viewing fixture;
they must not be committed or shipped.
"""
import argparse
import os
import sys
import urllib.request

# The 29 movable joints of data/g1/g1.urdf, in document order. The dataset's
# documented column order is the same sequence (left leg, right leg, waist,
# left arm, right arm) — verified against the URDF rather than assumed. The
# replay path still maps by name, not by position, so a future reordering of
# either side cannot silently shift every joint by one.
JOINTS = [
    "left_hip_pitch_joint", "left_hip_roll_joint", "left_hip_yaw_joint",
    "left_knee_joint", "left_ankle_pitch_joint", "left_ankle_roll_joint",
    "right_hip_pitch_joint", "right_hip_roll_joint", "right_hip_yaw_joint",
    "right_knee_joint", "right_ankle_pitch_joint", "right_ankle_roll_joint",
    "waist_yaw_joint", "waist_roll_joint", "waist_pitch_joint",
    "left_shoulder_pitch_joint", "left_shoulder_roll_joint",
    "left_shoulder_yaw_joint", "left_elbow_joint", "left_wrist_roll_joint",
    "left_wrist_pitch_joint", "left_wrist_yaw_joint",
    "right_shoulder_pitch_joint", "right_shoulder_roll_joint",
    "right_shoulder_yaw_joint", "right_elbow_joint",
    "right_wrist_roll_joint", "right_wrist_pitch_joint",
    "right_wrist_yaw_joint",
]

SRC_FPS = 30.0
BASE_URL = ("https://huggingface.co/datasets/lvhaidong/"
            "LAFAN1_Retargeting_Dataset/resolve/main/g1/")


def load_csv(path):
    rows = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            vals = [float(x) for x in line.split(",")]
            if len(vals) != 7 + len(JOINTS):
                sys.exit(f"{path}: expected {7 + len(JOINTS)} columns, got {len(vals)}")
            rows.append(vals)
    if not rows:
        sys.exit(f"{path}: no frames")
    return rows


def lerp(a, b, t):
    return a + (b - a) * t


def slerp_shortest(qa, qb, t):
    """Linear quaternion blend on the shortest arc, then normalised.

    Frames are 33 ms apart, so the arc is small and a normalised lerp is
    indistinguishable from a slerp; the sign fix is what actually matters —
    without it a quaternion that flips sign spins the base a full turn.
    """
    dot = sum(x * y for x, y in zip(qa, qb))
    if dot < 0.0:
        qb = [-x for x in qb]
    q = [lerp(a, b, t) for a, b in zip(qa, qb)]
    n = sum(x * x for x in q) ** 0.5
    return [x / n for x in q] if n > 1e-9 else [0.0, 0.0, 0.0, 1.0]


def resample(rows, start_s, seconds, out_fps):
    """Sample the 30 fps source at the output rate.

    The studio advances a replay 3 frames per 0.12 s tick — 25 fps — and
    ignores the rollout's own dt. Writing at that rate is what makes the walk
    play at 1.0x speed instead of 1.2x.
    """
    out = []
    n = len(rows)
    total = seconds if seconds > 0 else (n / SRC_FPS) - start_s
    count = max(1, int(round(total * out_fps)))
    for k in range(count):
        t = start_s + k / out_fps
        src = t * SRC_FPS
        i = int(src)
        if i >= n - 1:
            break
        frac = src - i
        a, b = rows[i], rows[i + 1]
        pos = [lerp(a[j], b[j], frac) for j in range(3)]
        quat = slerp_shortest(a[3:7], b[3:7], frac)
        joints = [lerp(a[7 + j], b[7 + j], frac) for j in range(len(JOINTS))]
        out.append((pos + quat, joints))
    if not out:
        sys.exit("resampled to zero frames — check --start against clip length")
    return out


def write_rollout(frames, dt, path):
    names = ", ".join(f'"{n}"' for n in JOINTS)
    base = ",".join("[" + ",".join(f"{v:.5f}" for v in b) + "]" for b, _ in frames)
    joints = ",".join("[" + ",".join(f"{v:.5f}" for v in j) + "]" for _, j in frames)
    # Same flat schema as zealot's rollout writer, and the same hand parser
    # reads it: dt, joint_names, resets, base, joints. `resets` is empty
    # because nothing terminated — a motion capture clip never falls over, and
    # claiming a reset would make the studio call the rollout collapsed.
    with open(path, "w") as f:
        f.write('{\n  "dt": %.4f,\n  "joint_names": [%s],\n  "resets": [],\n'
                '  "base": [%s],\n  "joints": [%s]\n}\n'
                % (dt, names, base, joints))


def write_rec(name, scene, frames, dist, rel_rollout, path):
    with open(path, "w") as f:
        f.write('{\n  "name": "%s",\n  "scene": "%s",\n  "frames": %d,\n'
                '  "resets": 0,\n  "distance": %.5f,\n  "rollout": "%s"\n}\n'
                % (name, scene, frames, dist, rel_rollout))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default="/Users/ychen/home/dorobot/dorobot-nexus",
                    help="dorobot-nexus checkout; recordings land under its scenes/")
    ap.add_argument("--clip", default="walk1_subject1", help="clip name in the g1/ set")
    ap.add_argument("--csv", help="use a local CSV instead of downloading")
    ap.add_argument("--name", help="recording name (default: lafan1-<clip>)")
    ap.add_argument("--scene", default="baseline", help="scene tag for the recording")
    ap.add_argument("--start", type=float, default=0.0, help="seconds into the clip")
    ap.add_argument("--seconds", type=float, default=20.0, help="0 for the whole clip")
    ap.add_argument("--fps", type=float, default=25.0, help="output rate")
    a = ap.parse_args()

    csv = a.csv
    if not csv:
        cache = os.path.join(os.path.dirname(os.path.abspath(__file__)), ".lafan1-cache")
        os.makedirs(cache, exist_ok=True)
        csv = os.path.join(cache, f"{a.clip}.csv")
        if not os.path.exists(csv) or os.path.getsize(csv) == 0:
            url = BASE_URL + a.clip + ".csv"
            print(f"fetching {url}")
            urllib.request.urlretrieve(url, csv)

    rows = load_csv(csv)
    frames = resample(rows, a.start, a.seconds, a.fps)
    name = a.name or f"lafan1-{a.clip}"

    out_dir = os.path.join(a.repo, "scenes", "recordings")
    os.makedirs(out_dir, exist_ok=True)
    slug = "".join(c if c.isalnum() or c in "-_" else "-" for c in name.lower())
    rollout_abs = os.path.join(out_dir, f"{slug}.rollout.json")
    # Stored relative to the repo: the studio composes NEXUS_REPO + this path,
    # so an absolute one would concatenate into nonsense.
    rollout_rel = os.path.join("scenes", "recordings", f"{slug}.rollout.json")

    (x0, y0), (x1, y1) = (frames[0][0][0], frames[0][0][1]), (frames[-1][0][0], frames[-1][0][1])
    dist = ((x1 - x0) ** 2 + (y1 - y0) ** 2) ** 0.5

    write_rollout(frames, 1.0 / a.fps, rollout_abs)
    write_rec(name, a.scene, len(frames), dist, rollout_rel,
              os.path.join(out_dir, f"{slug}.rec.json"))

    print(f"{name}: {len(frames)} frames @ {a.fps:g} fps "
          f"({len(frames) / a.fps:.1f}s), travels {dist:.2f} m")
    print(f"  {rollout_abs}")
    print(f"  {os.path.join(out_dir, f'{slug}.rec.json')}")


if __name__ == "__main__":
    main()
