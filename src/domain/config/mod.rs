//! Configuration types for Atoma.
//!
//! Organized into three sub-modules by concern:
//! - `atoma_toml`: atoma.toml profile and defaults
//! - `orchestration`: orchestration.json workflow config
//! - `shell_guard`: dangerous command blocking patterns

mod atoma_toml;
mod orchestration;
mod shell_guard;

pub use atoma_toml::*;
pub use orchestration::*;
pub use shell_guard::*;