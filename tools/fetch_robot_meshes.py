#!/usr/bin/env python3
"""Fetch the mesh files a URDF references, but does not carry.

Meshes are large binary blobs — the Unitree G1's are 110 MB for the full set —
so the URDF is committed and the meshes are fetched. Only the meshes a given
URDF actually references are downloaded: the G1's 29-DOF variant needs 35 of
the 167 files in upstream, which is 20 MB rather than 110 MB.

    tools/fetch_robot_meshes.py g1

Sources are declared per robot below. Re-running is cheap: files already present
at non-zero size are left alone.
"""
import concurrent.futures
import os
import re
import sys
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# robot -> (urdf relative to repo root, base URL for its mesh directory)
SOURCES = {
    "g1": (
        "data/g1/g1.urdf",
        "https://raw.githubusercontent.com/unitreerobotics/unitree_ros"
        "/master/robots/g1_description/meshes/",
    ),
}


def fetch(robot: str) -> int:
    try:
        urdf_rel, base = SOURCES[robot]
    except KeyError:
        print(f"unknown robot {robot!r}; known: {', '.join(sorted(SOURCES))}", file=sys.stderr)
        return 2

    urdf = os.path.join(ROOT, urdf_rel)
    if not os.path.exists(urdf):
        print(f"missing {urdf_rel} — the URDF is committed, so this is a broken checkout", file=sys.stderr)
        return 1

    with open(urdf) as f:
        refs = sorted({m for m in re.findall(r'filename="([^"]+)"', f.read())})
    if not refs:
        print(f"{urdf_rel} references no meshes; nothing to do")
        return 0

    urdf_dir = os.path.dirname(urdf)

    def one(ref: str):
        dest = os.path.join(urdf_dir, ref)
        if os.path.exists(dest) and os.path.getsize(dest) > 0:
            return ref, 0
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        urllib.request.urlretrieve(base + os.path.basename(ref), dest)
        return ref, os.path.getsize(dest)

    got, failed = [], []
    with concurrent.futures.ThreadPoolExecutor(8) as pool:
        futures = {pool.submit(one, r): r for r in refs}
        for fut in concurrent.futures.as_completed(futures):
            try:
                got.append(fut.result())
            except Exception as exc:  # noqa: BLE001 - report, do not abort the batch
                failed.append((futures[fut], exc))

    fetched = sum(1 for _, size in got if size)
    cached = sum(1 for _, size in got if not size)
    total = sum(size for _, size in got)
    print(f"{robot}: {fetched} fetched ({total / 1e6:.1f} MB), {cached} already present")
    for ref, exc in failed:
        print(f"  FAILED {ref}: {type(exc).__name__} {exc}", file=sys.stderr)
    return 1 if failed else 0


if __name__ == "__main__":
    targets = sys.argv[1:] or sorted(SOURCES)
    raise SystemExit(max(fetch(r) for r in targets))
