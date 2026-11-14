This is `platform/{src,build.rs,Cargo.toml}` from Makepad commit
`0cd882f793a4c05b4982c719bed49f5352682e9b`, with its upstream license.
Sibling dependencies use the original Git source and `dev` branch, resolved to
the same revision by the workspace Cargo.lock. No Makepad API upgrade is involved.

Runtime changes are confined to `src/os/apple/metal.rs` and native layer
setup in `src/os/apple/macos/macos.rs`. The geometry
vertex buffer is also bound to fragment slot 0, matching the generated Metal
shader entry point. On the current Apple GPU driver, the Microduck mesh shader retains
that argument. Previously Metal validation aborted with
`Fragment Function(fragment_main): missing Buffer binding at index 0 for vb[0]`,
and ordinary runs rendered an entirely black window when the 3D viewport drew.

The local Cargo patch makes the fix reproducible without modifying Cargo's
shared Git cache. Remove this vendored crate once the pinned upstream renderer
binds every declared fragment resource or removes unused fragment arguments.

Metal validation also exposed these invalid resource operations in the same path:

- `didModifyRange` was called on shared buffers. It is now guarded by the
  actual storage mode and only called for managed buffers.
- IOSurface-backed textures were created/imported with private storage on
  macOS. They now use shared storage, as required by Metal.
- Screenshot readback synchronized a shared texture as though it were managed.
  The invalid synchronization was removed; readback already occurs in the
  command-buffer completion handler.
- Depth render targets now declare shader-read usage, since the viewport
  composite samples them for edge detection and ambient occlusion.
- Studio can resize window geometry before its swapchain catches up. Screenshot
  dimensions are bounded by the actual readback texture to avoid out-of-bounds
  `getBytes` calls during that transition.

These changes preserve ordinary private render targets and geometry contents.
Runtime verification uses Makepad Studio's screenshots with `MTL_DEBUG_LAYER=1`.

Native windows connected to Studio allow drawable readback by disabling
`framebufferOnly` on their Metal layer. Unattached application windows retain
the original setting. Otherwise Studio's native screenshot request fails Metal
validation because it attempts to blit a framebuffer-only texture.
