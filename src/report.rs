//! What Perch prints when it hits something it never planned for.
//!
//! Perch's own failures are typed: [`crate::error::PerchError`] carries the
//! exit code a script reads, and the compiler checks every variant has one.
//! Handing those to a type-erased report would trade that for a downcast, and
//! a backtrace is the wrong thing to print at a command people put in a shell
//! prompt.
//!
//! A panic is different. It is a bug, and a bug deserves a report worth
//! pasting: what version, on what platform, where to send it, and how to get
//! the backtrace if the first run did not carry one.

const ISSUES: &str = "https://github.com/mschieller/perch/issues";

/// Adds Perch's own section to whatever the runtime already prints for a panic.
///
/// The default hook stays underneath rather than being replaced: it already
/// prints the payload, the location, and — when `RUST_BACKTRACE` is set — the
/// backtrace, and reimplementing that would only be a worse copy of it. What is
/// added is the part a bug report needs and the runtime has no way to know.
pub fn install_panic_hook() {
    let runtime = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        runtime(panic);
        eprintln!(
            "{}",
            bug_report(std::env::var_os("RUST_BACKTRACE").is_some())
        );
    }));
}

/// The section Perch adds, as one block of text.
///
/// A separate function because it is the part worth being sure of: a panic hook
/// runs once, on the worst day somebody is having with this tool, and is not
/// somewhere to find out that a line was wrong.
fn bug_report(backtrace_was_asked_for: bool) -> String {
    let mut said = format!(
        "\nThis is a bug in Perch {} ({} {}). Please report it, with everything \
         above, at {ISSUES}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    if !backtrace_was_asked_for {
        said.push_str(
            "\nRunning the same command again with RUST_BACKTRACE=1 set adds a \
             backtrace, which is the part that usually says where to look.",
        );
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_says_what_to_send_and_where() {
        let said = bug_report(false);
        assert!(said.contains(env!("CARGO_PKG_VERSION")), "{said}");
        assert!(said.contains(ISSUES), "{said}");
        assert!(said.contains("RUST_BACKTRACE=1"), "{said}");
    }

    /// Nobody needs telling to do the thing they have already done.
    #[test]
    fn a_run_that_already_asked_for_a_backtrace_is_not_asked_again() {
        assert!(!bug_report(true).contains("RUST_BACKTRACE"));
    }
}
