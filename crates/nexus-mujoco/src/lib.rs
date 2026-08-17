//! The mjvScene bridge: load an MJCF with the real libmujoco and hand its
//! abstract visualization scene — posed geoms, meshes, heightfields — to a
//! renderer that is not MuJoCo's.
//!
//! Nothing here links MuJoCo. At build time a C shim is compiled against the
//! wheel's headers (the only place struct layouts are known) into its own
//! dylib with undefined `mj_*` symbols; at runtime libmujoco is dlopen'd
//! GLOBAL first and the shim after it, so those symbols resolve inside the
//! process. A machine without MuJoCo builds fine and answers
//! [`why_unavailable`] instead, the same stance nexus-engine takes toward
//! its optional backends.

use libloading::Library;
use std::ffi::{c_char, c_double, c_float, c_int, CString};
use std::sync::OnceLock;

// Baked by build.rs; empty when MuJoCo was not found at build time.
const SHIM_PATH: &str = env!("NEXUS_MJSHIM");
const DYLIB_PATH: &str = env!("NEXUS_MUJOCO_DYLIB");

#[repr(C)]
struct NxCtx {
    _opaque: [u8; 0],
}

struct Bridge {
    // Held so the images stay mapped; the shim resolves mj_* out of _mujoco.
    _mujoco: libloading::os::unix::Library,
    _shim: Library,
    load: unsafe extern "C" fn(*const c_char, *mut c_char, c_int) -> *mut NxCtx,
    free: unsafe extern "C" fn(*mut NxCtx),
    nq: unsafe extern "C" fn(*mut NxCtx) -> c_int,
    nkey: unsafe extern "C" fn(*mut NxCtx) -> c_int,
    reset_keyframe: unsafe extern "C" fn(*mut NxCtx, c_int) -> c_int,
    set_qpos: unsafe extern "C" fn(*mut NxCtx, *const c_double, c_int) -> c_int,
    update: unsafe extern "C" fn(*mut NxCtx) -> c_int,
    geom: unsafe extern "C" fn(*mut NxCtx, c_int, *mut c_float, *mut c_int, *mut c_int, *mut c_int) -> c_int,
    nmesh: unsafe extern "C" fn(*mut NxCtx) -> c_int,
    mesh_counts: unsafe extern "C" fn(*mut NxCtx, c_int, *mut c_int, *mut c_int, *mut c_int) -> c_int,
    mesh_arrays: unsafe extern "C" fn(*mut NxCtx, c_int, *mut c_float, *mut c_int, *mut c_float, *mut c_int) -> c_int,
    nhfield: unsafe extern "C" fn(*mut NxCtx) -> c_int,
    hfield_counts: unsafe extern "C" fn(*mut NxCtx, c_int, *mut c_int, *mut c_int, *mut c_double) -> c_int,
    hfield_data: unsafe extern "C" fn(*mut NxCtx, c_int, *mut c_float) -> c_int,
}

unsafe impl Send for Bridge {}
unsafe impl Sync for Bridge {}

fn bridge() -> Result<&'static Bridge, String> {
    static B: OnceLock<Result<Bridge, String>> = OnceLock::new();
    B.get_or_init(|| {
        if SHIM_PATH.is_empty() {
            return Err("built without MuJoCo (headers not found; set NEXUS_MUJOCO_INCLUDE / NEXUS_MUJOCO_LIB and rebuild)".into());
        }
        let dylib = std::env::var("NEXUS_MUJOCO_LIB").unwrap_or_else(|_| DYLIB_PATH.into());
        // GLOBAL so the shim's lazy mj_* references can resolve against it.
        let mujoco = unsafe {
            libloading::os::unix::Library::open(
                Some(&dylib),
                libloading::os::unix::RTLD_NOW | libloading::os::unix::RTLD_GLOBAL,
            )
        }
        .map_err(|e| format!("dlopen {dylib}: {e}"))?;
        let shim = unsafe { Library::new(SHIM_PATH) }.map_err(|e| format!("dlopen shim: {e}"))?;
        macro_rules! sym {
            ($name:literal) => {
                *unsafe { shim.get($name) }.map_err(|e| format!("shim symbol {:?}: {e}", $name))?
            };
        }
        Ok(Bridge {
            load: sym!(b"nx_load"),
            free: sym!(b"nx_free"),
            nq: sym!(b"nx_nq"),
            nkey: sym!(b"nx_nkey"),
            reset_keyframe: sym!(b"nx_reset_keyframe"),
            set_qpos: sym!(b"nx_set_qpos"),
            update: sym!(b"nx_update"),
            geom: sym!(b"nx_geom"),
            nmesh: sym!(b"nx_nmesh"),
            mesh_counts: sym!(b"nx_mesh_counts"),
            mesh_arrays: sym!(b"nx_mesh_arrays"),
            nhfield: sym!(b"nx_nhfield"),
            hfield_counts: sym!(b"nx_hfield_counts"),
            hfield_data: sym!(b"nx_hfield_data"),
            _mujoco: mujoco,
            _shim: shim,
        })
    })
    .as_ref()
    .map_err(|e| e.clone())
}

/// `None` when the bridge can run here; otherwise the reason it cannot.
pub fn why_unavailable() -> Option<String> {
    bridge().err()
}

/// What a geom is, reduced to what a renderer needs.
#[derive(Clone, Debug, PartialEq)]
pub enum GeomKind {
    /// size = (half_x, half_y); 0 means infinite (render a large quad).
    Plane { half: [f32; 2] },
    /// dataid-resolved heightfield.
    Hfield { id: i32 },
    Sphere { r: f32 },
    /// Along local Z; half-length excludes the caps.
    Capsule { r: f32, half_len: f32 },
    Ellipsoid { half: [f32; 3] },
    /// Along local Z.
    Cylinder { r: f32, half_len: f32 },
    Box { half: [f32; 3] },
    /// dataid-resolved mesh (already divided by 2 — mjv doubles it for hulls).
    Mesh { id: i32 },
    Other(i32),
}

/// One posed geom out of mjvScene, in MuJoCo's Z-up world.
#[derive(Clone, Debug)]
pub struct Geom {
    pub kind: GeomKind,
    pub pos: [f32; 3],
    /// Row-major 3x3 rotation.
    pub mat: [f32; 9],
    pub rgba: [f32; 4],
}

/// CPU mesh: flat xyz positions, matching normals, triangle indices.
#[derive(Clone, Debug, Default)]
pub struct CpuMesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
}

pub struct Model {
    ctx: *mut NxCtx,
}

unsafe impl Send for Model {}

impl Drop for Model {
    fn drop(&mut self) {
        if let Ok(b) = bridge() {
            unsafe { (b.free)(self.ctx) };
        }
    }
}

impl Model {
    pub fn load(path: &str) -> Result<Model, String> {
        let b = bridge()?;
        let c_path = CString::new(path).map_err(|_| "path contains NUL".to_string())?;
        let mut err = vec![0u8; 1024];
        let ctx = unsafe { (b.load)(c_path.as_ptr(), err.as_mut_ptr() as *mut c_char, err.len() as c_int) };
        if ctx.is_null() {
            let msg = err.split(|&c| c == 0).next().unwrap_or(&[]);
            return Err(format!("mj_loadXML: {}", String::from_utf8_lossy(msg)));
        }
        Ok(Model { ctx })
    }

    pub fn nq(&self) -> usize {
        unsafe { (bridge().unwrap().nq)(self.ctx) as usize }
    }

    pub fn nkey(&self) -> usize {
        unsafe { (bridge().unwrap().nkey)(self.ctx) as usize }
    }

    /// Pose from keyframe `k` (playground scenes carry a "home").
    pub fn reset_keyframe(&mut self, k: usize) -> bool {
        unsafe { (bridge().unwrap().reset_keyframe)(self.ctx, k as c_int) != 0 }
    }

    pub fn set_qpos(&mut self, q: &[f64]) {
        unsafe { (bridge().unwrap().set_qpos)(self.ctx, q.as_ptr(), q.len() as c_int) };
    }

    /// Refresh the abstract scene and return its geoms.
    pub fn scene(&mut self) -> Vec<Geom> {
        let b = bridge().unwrap();
        let n = unsafe { (b.update)(self.ctx) };
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            let mut f = [0f32; 19];
            let (mut ty, mut dataid, mut cat) = (0, 0, 0);
            if unsafe { (b.geom)(self.ctx, i, f.as_mut_ptr(), &mut ty, &mut dataid, &mut cat) } == 0 {
                continue;
            }
            let size = [f[12], f[13], f[14]];
            // mjtGeom order: 0 plane, 1 hfield, 2 sphere, 3 capsule,
            // 4 ellipsoid, 5 cylinder, 6 box, 7 mesh.
            let kind = match ty {
                0 => GeomKind::Plane { half: [size[0], size[1]] },
                1 => GeomKind::Hfield { id: dataid },
                2 => GeomKind::Sphere { r: size[0] },
                3 => GeomKind::Capsule { r: size[0], half_len: size[2] },
                4 => GeomKind::Ellipsoid { half: size },
                5 => GeomKind::Cylinder { r: size[0], half_len: size[2] },
                6 => GeomKind::Box { half: size },
                7 => GeomKind::Mesh { id: dataid / 2 },
                t => GeomKind::Other(t),
            };
            out.push(Geom {
                kind,
                pos: [f[0], f[1], f[2]],
                mat: [f[3], f[4], f[5], f[6], f[7], f[8], f[9], f[10], f[11]],
                rgba: [f[15], f[16], f[17], f[18]],
            });
        }
        out
    }

    pub fn nmesh(&self) -> usize {
        unsafe { (bridge().unwrap().nmesh)(self.ctx) as usize }
    }

    /// A model mesh as renderable triangle soup. Positions and normals live
    /// in separate index spaces in mjModel (`mesh_face` vs `mesh_facenormal`),
    /// so corners are expanded rather than guessed into one index space.
    pub fn mesh(&self, id: usize) -> Option<CpuMesh> {
        let b = bridge().unwrap();
        let (mut nv, mut nf, mut nn) = (0, 0, 0);
        if unsafe { (b.mesh_counts)(self.ctx, id as c_int, &mut nv, &mut nf, &mut nn) } == 0 {
            return None;
        }
        // Absurd counts mean a header/dylib mismatch reading garbage offsets;
        // refuse loudly instead of allocating by the gigabyte.
        if nv <= 0 || nf <= 0 || nn < 0 || nv > 10_000_000 || nf > 10_000_000 || nn > 10_000_000 {
            eprintln!("nexus-mujoco: implausible mesh {id} counts (nv={nv} nf={nf} nn={nn}) — header/dylib mismatch?");
            return None;
        }
        let mut vert = vec![0f32; 3 * nv as usize];
        let mut face = vec![0i32; 3 * nf as usize];
        let mut norm = vec![0f32; 3 * nn.max(1) as usize];
        let mut fnorm = vec![0i32; 3 * nf as usize];
        if unsafe {
            (b.mesh_arrays)(self.ctx, id as c_int, vert.as_mut_ptr(), face.as_mut_ptr(), norm.as_mut_ptr(), fnorm.as_mut_ptr())
        } == 0
        {
            return None;
        }
        let corners = 3 * nf as usize;
        let mut out = CpuMesh {
            positions: Vec::with_capacity(3 * corners),
            normals: Vec::with_capacity(3 * corners),
            indices: (0..corners as u32).collect(),
        };
        for k in 0..corners {
            let vi = face[k];
            let ni = fnorm[k];
            if vi < 0 || vi >= nv || (nn > 0 && (ni < 0 || ni >= nn)) {
                eprintln!("nexus-mujoco: mesh {id} corner {k} indexes out of range (vi={vi} ni={ni}) — refusing");
                return None;
            }
            let vi = vi as usize;
            out.positions.extend_from_slice(&vert[3 * vi..3 * vi + 3]);
            if nn > 0 {
                let ni = ni as usize;
                out.normals.extend_from_slice(&norm[3 * ni..3 * ni + 3]);
            } else {
                out.normals.extend_from_slice(&[0.0, 0.0, 1.0]);
            }
        }
        Some(out)
    }

    /// A heightfield as a mesh in its own frame: x spans ±radius_x, y spans
    /// ±radius_y, z = data * z_top (data is normalized to [0,1]).
    pub fn hfield(&self, id: usize) -> Option<CpuMesh> {
        let b = bridge().unwrap();
        let (mut nrow, mut ncol) = (0, 0);
        let mut size = [0f64; 4];
        if unsafe { (b.hfield_counts)(self.ctx, id as c_int, &mut nrow, &mut ncol, size.as_mut_ptr()) } == 0 {
            return None;
        }
        if nrow < 2 || ncol < 2 || (nrow as i64) * (ncol as i64) > 50_000_000 {
            eprintln!("nexus-mujoco: implausible hfield {id} ({nrow}x{ncol}) — header/dylib mismatch?");
            return None;
        }
        let (nrow, ncol) = (nrow as usize, ncol as usize);
        let mut data = vec![0f32; nrow * ncol];
        if unsafe { (b.hfield_data)(self.ctx, id as c_int, data.as_mut_ptr()) } == 0 {
            return None;
        }
        let (rx, ry, ztop) = (size[0] as f32, size[1] as f32, size[2] as f32);
        let mut m = CpuMesh::default();
        let z = |r: usize, c: usize| data[r * ncol + c] * ztop;
        for r in 0..nrow {
            for c in 0..ncol {
                let x = -rx + 2.0 * rx * c as f32 / (ncol - 1) as f32;
                let y = -ry + 2.0 * ry * r as f32 / (nrow - 1) as f32;
                m.positions.extend_from_slice(&[x, y, z(r, c)]);
                // central differences for the normal
                let dzdx = (z(r, (c + 1).min(ncol - 1)) - z(r, c.saturating_sub(1)))
                    / (2.0 * 2.0 * rx / (ncol - 1) as f32);
                let dzdy = (z((r + 1).min(nrow - 1), c) - z(r.saturating_sub(1), c))
                    / (2.0 * 2.0 * ry / (nrow - 1) as f32);
                let len = (dzdx * dzdx + dzdy * dzdy + 1.0).sqrt();
                m.normals.extend_from_slice(&[-dzdx / len, -dzdy / len, 1.0 / len]);
            }
        }
        for r in 0..nrow - 1 {
            for c in 0..ncol - 1 {
                let i = (r * ncol + c) as u32;
                let (a, b2, cc, d) = (i, i + 1, i + ncol as u32, i + ncol as u32 + 1);
                m.indices.extend_from_slice(&[a, b2, d, a, d, cc]);
            }
        }
        Some(m)
    }
}

/// Unit-ish primitives, all along local Z where MuJoCo puts them. Sized
/// primitives that scale badly (capsules) are generated per-size by the
/// caller; the rest are unit meshes scaled per instance.
pub mod prim {
    use super::CpuMesh;

    /// UV sphere, radius 1.
    pub fn unit_sphere(rings: usize, segs: usize) -> CpuMesh {
        let mut m = CpuMesh::default();
        for r in 0..=rings {
            let phi = std::f32::consts::PI * r as f32 / rings as f32;
            for s in 0..=segs {
                let th = 2.0 * std::f32::consts::PI * s as f32 / segs as f32;
                let (x, y, z) = (phi.sin() * th.cos(), phi.sin() * th.sin(), phi.cos());
                m.positions.extend_from_slice(&[x, y, z]);
                m.normals.extend_from_slice(&[x, y, z]);
            }
        }
        let w = segs + 1;
        for r in 0..rings {
            for s in 0..segs {
                let i = (r * w + s) as u32;
                m.indices.extend_from_slice(&[i, i + 1, i + w as u32 + 1, i, i + w as u32 + 1, i + w as u32]);
            }
        }
        m
    }

    /// Box with half-extents 1.
    pub fn unit_box() -> CpuMesh {
        let mut m = CpuMesh::default();
        let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
        ];
        for (n, u, v) in faces {
            let base = (m.positions.len() / 3) as u32;
            for (su, sv) in [(-1f32, -1f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                for k in 0..3 {
                    m.positions.push(n[k] + u[k] * su + v[k] * sv);
                }
                m.normals.extend_from_slice(&n);
            }
            m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        m
    }

    /// Cylinder along Z: radius 1, half-length 1, capped.
    pub fn unit_cylinder(segs: usize) -> CpuMesh {
        let mut m = CpuMesh::default();
        for s in 0..=segs {
            let th = 2.0 * std::f32::consts::PI * s as f32 / segs as f32;
            let (x, y) = (th.cos(), th.sin());
            m.positions.extend_from_slice(&[x, y, -1.0, x, y, 1.0].map(|v| v));
            m.normals.extend_from_slice(&[x, y, 0.0, x, y, 0.0]);
        }
        for s in 0..segs {
            let i = (2 * s) as u32;
            m.indices.extend_from_slice(&[i, i + 2, i + 3, i, i + 3, i + 1]);
        }
        for &(z, nz) in &[(1.0f32, 1.0f32), (-1.0, -1.0)] {
            let center = (m.positions.len() / 3) as u32;
            m.positions.extend_from_slice(&[0.0, 0.0, z]);
            m.normals.extend_from_slice(&[0.0, 0.0, nz]);
            let start = (m.positions.len() / 3) as u32;
            for s in 0..=segs {
                let th = 2.0 * std::f32::consts::PI * s as f32 / segs as f32;
                m.positions.extend_from_slice(&[th.cos(), th.sin(), z]);
                m.normals.extend_from_slice(&[0.0, 0.0, nz]);
            }
            for s in 0..segs as u32 {
                if nz > 0.0 {
                    m.indices.extend_from_slice(&[center, start + s, start + s + 1]);
                } else {
                    m.indices.extend_from_slice(&[center, start + s + 1, start + s]);
                }
            }
        }
        m
    }

    /// Capsule along Z for a specific (radius, half-length) — caps do not
    /// survive non-uniform scaling, so these are per-size.
    pub fn capsule(r: f32, half_len: f32, rings: usize, segs: usize) -> CpuMesh {
        let mut m = CpuMesh::default();
        // hemispheres + tube from one sphere sweep, shifted at the equator
        for hr in 0..=(2 * rings) {
            let phi = std::f32::consts::PI * hr as f32 / (2 * rings) as f32;
            let zoff = if hr <= rings { half_len } else { -half_len };
            for s in 0..=segs {
                let th = 2.0 * std::f32::consts::PI * s as f32 / segs as f32;
                let (nx, ny, nz) = (phi.sin() * th.cos(), phi.sin() * th.sin(), phi.cos());
                m.positions.extend_from_slice(&[r * nx, r * ny, r * nz + zoff]);
                m.normals.extend_from_slice(&[nx, ny, nz]);
            }
        }
        let w = segs + 1;
        for band in 0..(2 * rings) {
            for s in 0..segs {
                let i = (band * w + s) as u32;
                m.indices.extend_from_slice(&[i, i + 1, i + w as u32 + 1, i, i + w as u32 + 1, i + w as u32]);
            }
        }
        m
    }

    /// Plane quad in XY, half-extents (hx, hy), normal +Z.
    pub fn plane(hx: f32, hy: f32) -> CpuMesh {
        let mut m = CpuMesh::default();
        for (x, y) in [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)] {
            m.positions.extend_from_slice(&[x, y, 0.0]);
            m.normals.extend_from_slice(&[0.0, 0.0, 1.0]);
        }
        m.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        m
    }
}
