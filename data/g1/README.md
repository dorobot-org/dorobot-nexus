# Unitree G1 — 29 DOF

`g1.urdf` is `g1_29dof_rev_1_0.urdf` from Unitree's
[`unitree_ros`](https://github.com/unitreerobotics/unitree_ros/tree/master/robots/g1_description),
BSD-3-Clause, copyright Unitree Robotics.

The 29-DOF variant is the one to use here: it matches the body
[zealot](https://github.com/haixuanTao/zealot) trains whole-body control against.

Verified in `makepad-urdf-player`: **39 links, 35 with meshes, 29 movable joints.**

## Meshes are not committed

`meshes/` is gitignored and fetched on demand:

```
tools/fetch_robot_meshes.py g1
```

Upstream ships 167 meshes totalling 110 MB, covering every G1 variant including
the dexterous hands. This URDF references 35 of them — 20 MB — and the script
downloads only those. Committing binaries that large for one robot would be
paid for by every clone forever, whether or not it ever renders a G1.

## Other variants

`unitree_ros` also carries 23-DOF, waist-locked, and several hand
configurations. Adding one means dropping its URDF in a sibling directory and
adding an entry to `SOURCES` in the fetch script — the mesh set is derived from
whatever the URDF references, so nothing else needs to change.
