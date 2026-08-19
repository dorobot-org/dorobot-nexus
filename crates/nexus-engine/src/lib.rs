//! The engine: everything that computes.
//!
//! This crate is the canonical backend. The learner, the vectorised
//! environment, checkpoints and their provenance, the robustness sweep,
//! sim-to-sim, the probe, and the zealot client all live here — and none of
//! them knows a UI exists. `nexus-studio` draws this engine and cannot be
//! depended on from here. The arrow points one way on purpose: a capability
//! the console needs is added here and called, never reimplemented up there.
//!
//! There were two consoles until `nexus-app` was retired. That it could be
//! deleted without moving any compute is the property this layout was for —
//! every capability it had was already here, reachable from [`cli`], and the
//! only thing lost with it was one screen's worth of drawing.
//!
//! **It has no dependencies.** Not "few" — none. A clean clone compiles this
//! crate with nothing but a Rust toolchain, which is what lets other products
//! consume the artifact schema without inheriting a UI stack, and what keeps
//! the physics honest about being physics. The optional `zealot` feature adds
//! none either: zealot is driven as a subprocess, so the flag compiles a client
//! that speaks to it, not a library that links it.
//!
//! [`cli`] is part of that promise rather than an exception to it — the
//! headless surfaces are pure std, so they ship with the engine and both
//! consoles inherit them.

pub mod ckpt;
pub mod cli;
pub mod crosssim;
pub mod env;
pub mod json;
pub mod mujoco;
pub mod probe;
pub mod rl;
pub mod rng;
pub mod scene;
pub mod sweep;
pub mod trainer;

/// Always compiled — it is pure std and pulls in nothing. The `zealot` feature
/// decides whether a console *prefers* this backend, not whether it exists.
#[allow(dead_code)]
pub mod zealot;
