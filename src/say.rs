//! What Perch says, and how it words what it says.
//!
//! The other half of [`crate::ask`]: one place for the rules a line is written
//! under, because a second spelling of any of them is one surface saying what
//! its neighbor would not. Everything a terminal would act on is taken out
//! (ADR nothing-drawn-is-obeyed), a document is pretty-printed and left alone,
//! and a noun agrees with the count in front of it.
//!
//! A remark from below is `Host::note` rather than any of this, and what a
//! *figure* reads as is [`crate::utilization`].

use std::io::Write;

use crate::error::{PerchError, Result};

/// A command's output going nowhere — a closed pipe, most often. Not a failure
/// of the thing the command was asked to do, but the command cannot finish
/// saying what it did.
pub fn failed(err: std::io::Error) -> PerchError {
    PerchError::Other(format!("Perch could not write its output: {err}"))
}

/// One line to the person running the command, with whatever a terminal would
/// act on taken out. At the writer, because a
/// sentence is a `format!` and there is no column to hang the question on: a
/// plan, a Quota Window's name and a path read out of a unit file reach a
/// person this way, and nobody chose any of them.
pub fn line(out: &mut dyn Write, line: &str) -> Result<()> {
    writeln!(out, "{}", crate::host::Shown::in_prose(line)).map_err(failed)
}

/// One document to whatever is parsing the command, which is what `--json`
/// means everywhere it is offered. Here rather than at the call sites because a
/// rule about machine-readable output — pretty or compact, stdout or elsewhere —
/// is a rule with one place to hold it.
pub fn json(out: &mut dyn Write, document: &serde_json::Value) -> Result<()> {
    let rendered =
        serde_json::to_string_pretty(document).map_err(|err| PerchError::Other(err.to_string()))?;
    // Written rather than said: serde has already escaped a control character as
    // six characters a parser wants, and a second pass would take those six out
    // of a document a script is reading.
    writeln!(out, "{rendered}").map_err(failed)
}

/// An Account as the two lines announcing a new one name it: the address, and
/// whatever of the organization and the plan is known, parenthesized.
///
/// One place, because an Adoption and an Add are the same news about the same
/// thing and a detail added to one spelling is missing from the other.
pub fn described(email: &str, organization: Option<&str>, plan: Option<&str>) -> String {
    let details: Vec<&str> = [organization, plan].into_iter().flatten().collect();
    match details.is_empty() {
        true => email.to_string(),
        false => format!("{email} ({})", details.join(", ")),
    }
}

/// A count of Accounts, with the noun agreeing with it. One place, because
/// "1 Accounts" is the kind of thing that ships and stays shipped, and seven call
/// sites spelling their own is seven chances at it.
pub fn accounts(count: usize) -> String {
    counted(count, "Account")
}

/// The same for the two nouns a Purge counts where the Registry named no Account.
pub fn profiles(count: usize) -> String {
    counted(count, "Profile")
}

pub fn credentials(count: usize) -> String {
    counted(count, "Credential")
}

pub fn groups(count: usize) -> String {
    counted(count, "Group")
}

/// Not a noun of Perch's: what `perch config` says about a command line it was
/// given too many of.
pub fn words(count: usize) -> String {
    counted(count, "word")
}

fn counted(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        _ => format!("{count} {noun}s"),
    }
}
