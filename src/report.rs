//! What Perch prints when it hits something it never planned for.
//!
//! Perch's own failures are typed: [`crate::error::PerchError`] carries the
//! exit code a script reads, and the compiler checks every variant has one.
//! Handing those to a type-erased report would trade that for a downcast, and
//! a backtrace is the wrong thing to print at a command people put in a shell
//! prompt.
//!
//! A panic is different. It is a bug, and a bug deserves a report worth
//! pasting — so colour-eyre's panic hook goes in, and its error hook stays out.

/// Installs colour-eyre's panic hook, and only its panic hook.
pub fn install_panic_hook() {
    let (panic_hook, _eyre_hook) = color_eyre::config::HookBuilder::default()
        .panic_section(
            "This is a bug in Perch. Please report it, with the report above, at \
             https://github.com/mschieller/perch/issues",
        )
        .into_hooks();
    panic_hook.install();
}
