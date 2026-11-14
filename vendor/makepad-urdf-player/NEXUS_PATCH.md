# Nexus renderer patch

Source: dorobot-org/makepad-urdf-viewer commit `283d303df6d3e158bac2c281a190ac1f749b5bd5`. Its Cargo manifest declares MIT licensing; this source revision contains no separate license file. The root Cargo patch selects this local copy; upstream checkouts remain unchanged.

The original loader averages the three URDF mesh scale values and applies a uniform scale. This distorts models with nonuniform scaling. `load_mesh_for_visual` now applies each axis independently, transforms normals by inverse transpose, updates bounds and reverses triangle winding for a reflection. Invalid zero/nonfinite scales fail explicitly. Nexus tests exercise positions, normals, bounds, winding and the actual URDF loader, followed by a native viewport capture of the transferred robot fixture.

The importer preserves material and texture resources in portable bundles. The existing viewer shades geometry with URDF/material colors; this patch does not add texture rendering.

Prismatic joints now participate in joint mapping and replay. Their scalar coordinate translates along the joint's local axis after the URDF origin transform; it does not rotate the link. The original code marked them unsupported and omitted them from movable joints. Fixed joints now use only their origin transform, avoiding an unnecessary axis rotation.

The renderer now passes mutable draw variables to `Cx2d::add_instance`, as required by the Makepad revision used by OctoSense/jit.
The composite shader uses the current `texture.sample` API and normalized RGBA channels.
