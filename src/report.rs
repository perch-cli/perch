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

const ISSUES: &str = "https://github.com/perch-cli/perch/issues";

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
        eprintln!("{}", bug_report(backtrace_was_asked_for()));
    }));
}

/// Whether the runtime just printed a backtrace.
///
/// Split from the reading of the variable so the rule can be tested without a
/// test mutating the environment of a process that is running three hundred
/// others in parallel — and the rule is the part that was wrong.
fn backtrace_was_asked_for() -> bool {
    asked_for(std::env::var_os("RUST_BACKTRACE").as_deref())
}

/// The runtime's own reading of `RUST_BACKTRACE`: nought is off, anything else
/// is on, unset is off.
///
/// The variable's mere presence is not the question. `RUST_BACKTRACE=0` is how
/// the runtime is told *not* to print one, so reading it as "already asked for"
/// withheld the suggestion from exactly the person who has no backtrace to
/// send.
fn asked_for(set: Option<&std::ffi::OsStr>) -> bool {
    set.is_some_and(|asked| asked != "0")
}

/// What every report of a Perch bug has to carry: which Perch, on what, and
/// where to send it.
///
/// Shared with [`crate::registry::save`], which declines to write a registry no
/// later command could read — not a panic, but a bug all the same, and one the
/// person in front of it can do nothing about. What each of them adds after
/// this sentence is their own: a panic can suggest a backtrace, and a refusal
/// says what it did not do instead.
pub(crate) fn this_is_a_bug() -> String {
    format!(
        "This is a bug in Perch {} ({} {}). Please report it, with everything \
         above, at {ISSUES}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// The section Perch adds, as one block of text.
///
/// A separate function because it is the part worth being sure of: a panic hook
/// runs once, on the worst day somebody is having with this tool, and is not
/// somewhere to find out that a line was wrong.
fn bug_report(backtrace_was_asked_for: bool) -> String {
    let mut said = format!("\n{}", this_is_a_bug());
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

    /// And the value that means "do not print one" is not the thing already
    /// done. `RUST_BACKTRACE=0` is how the runtime is told to stay quiet, so
    /// reading the variable's mere presence as agreement withheld the
    /// suggestion from the one person who has no backtrace to send.
    #[test]
    fn nought_is_the_value_that_asks_for_no_backtrace_at_all() {
        let set = |value: &str| asked_for(Some(std::ffi::OsStr::new(value)));

        assert!(!asked_for(None), "unset asks for none");
        assert!(!set("0"), "and nought is how the runtime is told not to");
        assert!(set("1"));
        assert!(set("full"));
    }
}
