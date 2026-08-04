//! The commands themselves. Each one takes a `&dyn Host` and a writer and
//! returns a `Result`, so behaviour tests drive the real code with no process,
//! no filesystem, and no keychain.

pub mod status;
