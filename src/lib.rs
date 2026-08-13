//! Library surface: the artifact schemas.
//!
//! The products around dorobot-nexus meet it at an artifact boundary — scene
//! files, recordings, checkpoints — not a code boundary. This lib target
//! codifies exactly that: it exposes the scene/recording schema module and
//! nothing else, and with `default-features = false` it compiles with no
//! dependencies beyond the standard library. The full application (makepad
//! UI, trainer, sweep) lives behind the default `app` feature.

pub mod scene;
