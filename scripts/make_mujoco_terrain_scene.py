#!/usr/bin/env python3
"""Build a MuJoCo sim-to-sim scene on zealot's training terrain.

Takes playground's flat feet-only G1 scene and replaces the ground plane with
a heightfield sized from a terrain_export `.hgrid.json` — the SAME grid the
zealot env collides with, so the MuJoCo cross-check runs on the terrain the
policy trained on rather than on flat ground.

The hfield asset is declared with nrow/ncol only (data left zero); the
sim2sim harness fills it float-exact from the same JSON at load
(S2S_HFIELD_JSON), avoiding 8-bit PNG quantisation entirely. The XML bakes
size (extent + elevation range) and geom position from the JSON, and the
harness asserts the two agree so a stale scene fails loudly.

Usage:
  make_mujoco_terrain_scene.py <hgrid.json> <out_scene.xml>

Python 3.9-compatible, stdlib only.
"""
import json
import os
import sys

FLAT = os.path.join(
    "mujoco-stack", "playground", "mujoco_playground", "_src", "locomotion",
    "g1", "xmls", "scene_mjx_feetonly_flat_terrain.xml",
)


def main():
    grid_path, out_path = sys.argv[1], sys.argv[2]
    with open(grid_path) as f:
        g = json.load(f)
    heights = g["heights"]
    nrow = len(heights)              # nodes along Y
    ncol = len(heights[0])           # nodes along X
    hs, x0, y0 = g["hs"], g["x0"], g["y0"]
    span_x = (ncol - 1) * hs
    span_y = (nrow - 1) * hs
    flat = [h for row in heights for h in row]
    hmin, hmax = min(flat), max(flat)
    zrange = max(hmax - hmin, 1e-6)
    cx = x0 + span_x / 2.0
    cy = y0 + span_y / 2.0

    with open(FLAT) as f:
        xml = f.read()

    # The hfield asset: extents are half-sizes; elevation spans [0, zrange]
    # above the geom frame, whose z sits at the grid minimum. 0.5 m of base
    # keeps the slab closed below the lowest node.
    asset = (
        '<hfield name="terrain" nrow="{nrow}" ncol="{ncol}" '
        'size="{rx:.4f} {ry:.4f} {zr:.4f} 0.5"/>'.format(
            nrow=nrow, ncol=ncol, rx=span_x / 2.0, ry=span_y / 2.0, zr=zrange
        )
    )
    xml = xml.replace("<asset>", "<asset>\n    " + asset, 1)

    # Replace the plane with the heightfield — and give the HFIELD the name
    # `floor`: the feet-only G1 model declares explicit contact pairs against
    # the geom named "floor" (g1_mjx_feetonly.xml <contact>), so any other
    # name leaves the terrain contactless and the robot falls straight
    # through onto whatever else exists. The visual apron plane under the
    # spawn side is deliberately NOT paired: MuJoCo planes collide as
    # INFINITE half-spaces whatever their size, and an active plane under the
    # terrain is exactly what the robot would land on after falling through.
    floor = '<geom name="floor" size="0 0 0.01" type="plane" material="groundplane"/>'
    terrain = (
        '<geom name="apron" size="{apron:.4f} {ry:.4f} 0.01" type="plane" '
        'material="groundplane" pos="{ax:.4f} 0 0" contype="0" conaffinity="0"/>\n'
        '    <geom name="floor" type="hfield" hfield="terrain" '
        'material="groundplane" pos="{cx:.4f} {cy:.4f} {z:.4f}"/>'.format(
            apron=max(x0, 1.0) / 2.0 + 1.0,
            ax=x0 / 2.0 - 1.0,
            ry=span_y / 2.0,
            cx=cx,
            cy=cy,
            z=hmin,
        )
    )
    assert floor in xml, "flat scene layout changed; update this builder"
    xml = xml.replace(floor, terrain, 1)

    with open(out_path, "w") as f:
        f.write(xml)
    print(
        "terrain scene: {}x{} nodes, x {:.1f}..{:.1f}, y {:.1f}..{:.1f}, "
        "z {:.3f}..{:.3f} -> {}".format(
            nrow, ncol, x0, x0 + span_x, y0, y0 + span_y, hmin, hmax, out_path
        )
    )


if __name__ == "__main__":
    main()
