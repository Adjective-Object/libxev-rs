//! Rust wrappers around the libxev fork's extended C API.
//!
//! Gated behind the `extended-api` feature. Add submodules here as the fork
//! grows new exports.

mod completion;
pub use completion::CompletionExt;

pub mod file;
pub use file::File;
