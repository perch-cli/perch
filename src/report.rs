//! What Perch prints when it hits something it never planned for.
//!
//! The second of Perch's two error idioms (ADR a-refusal-is-a-promise): an
//! expected failure is a typed [`crate::error::PerchError`] carrying an exit
//! code, and a panic is a bug. A bug deserves a report worth pasting — what
//! version, on what platform, where to send it, and how to get the backtrace
//! if the first run did not carry one.

/// Where a report of a Perch bug goes, for the panic hook here and for the
/// Triage that helps somebody write one.
pub(crate) const ISSUES: &str = "https://github.com/perch-cli/perch/issues";

/// The default hook stays underneath rather than being replaced — it already
/// prints the payload, the location and the backtrace — so what is added is
/// only the part a bug report needs and the runtime has no way to know.
pub fn install_panic_hook() {
    let runtime = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        runtime(panic);
        eprintln!("{}", bug_report(backtrace_was_asked_for()));
    }));
}

/// Whether the runtime just printed one. Split from the reading of the variable
/// so the rule can be tested without mutating the environment of a process
/// running the rest of the suite alongside it.
fn backtrace_was_asked_for() -> bool {
    asked_for(std::env::var_os("RUST_BACKTRACE").as_deref())
}

/// The runtime's own reading of `RUST_BACKTRACE`: nought is off, anything else
/// is on, unset is off. `RUST_BACKTRACE=0` is how the runtime is told *not* to
/// print one, so reading the variable's presence as agreement withholds the
/// suggestion from the one person with no backtrace to send.
fn asked_for(set: Option<&std::ffi::OsStr>) -> bool {
    set.is_some_and(|asked| asked != "0")
}

/// What every report of a Perch bug has to carry: which Perch, on what, and where
/// to send it. Shared with [`crate::registry::save`]'s refusal to write a Registry
/// no later command could read — not a panic, but a bug all the same. What each
/// adds after this sentence is its own.
pub(crate) fn this_is_a_bug() -> String {
    format!(
        "This is a bug in Perch {} ({} {}). Please report it, with everything \
         above and the output of `perch probe`, at {ISSUES}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// A function of its own so the text can be asserted: a panic hook runs once,
/// and is not somewhere to find out that a line was wrong.
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
        // A panic is when nobody knows what state the machine was in, which is
        // the one command that answers that.
        assert!(said.contains("perch probe"), "{said}");
    }

    #[test]
    fn a_run_that_already_asked_for_a_backtrace_is_not_asked_again() {
        assert!(!bug_report(true).contains("RUST_BACKTRACE"));
    }

    #[test]
    fn nought_is_the_value_that_asks_for_no_backtrace_at_all() {
        let set = |value: &str| asked_for(Some(std::ffi::OsStr::new(value)));

        assert!(!asked_for(None), "unset asks for none");
        assert!(!set("0"), "and nought is how the runtime is told not to");
        assert!(set("1"));
        assert!(set("full"));
    }
}
