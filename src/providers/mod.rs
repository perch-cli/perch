//! Provider implementations are accessible only through the shared contract.

mod claude;
mod codex;
/// The one exception, and it ships in no release: the gated suite asks the
/// installed `codex` whether it still answers what the adapter sends it.
#[cfg(feature = "your-machine")]
pub use codex::fixture as codex_fixture;
mod diagnostics;
pub mod provider;
mod sessions;
