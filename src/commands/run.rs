//! `perch run <target>` — a client against one Account's Profile, without a
//! Switch (ADR a-run-is-one-shot).
//!
//! One process is pointed at one Profile by setting `CLAUDE_CONFIG_DIR` for it
//! and nothing else, so a Run shares none of a Switch's machinery. Several Runs
//! coexist, which is why nothing here holds anything for as long as the client
//! lives — the Registry least of all. It is the one path where a Profile is a
//! live configuration directory, so the one path that Reconciles and the one
//! that makes a Profile Live. Perch's own remarks go to standard error, because
//! what the client says on stdout is the whole of what a Run says on stdout.

use std::io::Write;

use crate::adopt;
use crate::error::{PerchError, Result};
use crate::holdings;
use crate::host::Host;
use crate::registry::{self, Registry};
use crate::say;
use crate::switch;
use crate::{carry, probe, reconcile, target};

/// What a Run was asked for. `command` is text, like every other word Perch
/// takes: one that is not text is refused by the parser rather than mangled here.
#[derive(Debug, Clone, clap::Args)]
pub struct RunArgs {
    /// The Account to run as: its Alias, or its email address. A Group has
    /// no single meaning here, so naming one is refused.
    pub target: String,

    /// What to run, after a mandatory `--`: a program and its arguments, or
    /// Claude Code's own arguments where the first word is a flag. Nothing
    /// here is read by Perch, so a `--json` after `--` is the program's.
    #[arg(last = true, allow_hyphen_values = true, num_args = .., value_name = "COMMAND")]
    pub command: Vec<String>,
}

/// Launches a client against the named Account's Profile and reports the status
/// it exited with. The status comes back rather than being folded into success
/// or failure: a Run is a way of launching a program, and flattening what it
/// said would break every script that branches on it.
pub fn run(host: &dyn Host, args: RunArgs, out: &mut dyn Write) -> Result<i32> {
    // Read and let go, never held. A Run lasts as long as somebody's session,
    // and a Registry lock held across that would shut every other Perch out —
    // including the second Run this command exists to make possible.
    let registry = adopt::ensure_adopted(host)?;

    // A Group names a set of Accounts declared interchangeable, which is
    // nothing a Run can act on: there is no one Profile to point a process at.
    let found = target::resolve_account(&registry, &args.target)?;
    host.note(&found.matched);
    refuse_a_quarantined_account(&registry, &found.email)?;
    // Beside the Quarantine refusal, and for a reason of the same size: a
    // Profile two Accounts share holds one Credential, so the client runs as
    // whichever of them is in it while the line above named the other.
    switch::refuse_a_shared_profile(registry.held(&found.email)?, &registry)?;

    // Settled before anything is linked: where this is the Claude Code the probe
    // has to find, a machine without one is a refusal that should cost the
    // filesystem nothing.
    let launch = what_to_launch(host, &args.command)?;
    let profile = holdings::profile_dir_for(host, &found.email)?;
    let default_profile = holdings::the_default_profile(host)?;

    // Claimed before anything is linked: until the Marker exists nothing on the
    // machine knows this Run is happening, and a `perch remove` elsewhere would
    // be told the Profile is idle and delete it while this is linking into it.
    let _live = probe::claim(host, &profile)?;

    reconcile::reconcile(host, &default_profile.config_dir, &profile)?;

    // The one file Reconcile cannot link, because it holds the Account as well
    // as the person (ADR everything-but-the-account). It asks whether a Landing
    // is in flight rather than settling one, since a Run holds no Registry lock.
    let settled = registry::nothing_in_flight(&registry);

    carry::carry(
        host,
        &registry,
        &found.email,
        &default_profile,
        &profile,
        settled.as_ref(),
    );

    host.note(&launching(
        &registry,
        &found.email,
        &launch.said,
        settled.as_ref(),
    ));
    // Flushed before the client is handed the terminal: a command run before
    // this one may have left something in the buffer, and it would be delivered
    // after the output of the thing it was announcing.
    out.flush().map_err(say::failed)?;

    // The environment of this one process, and the whole of what makes the Run
    // a Run.
    let handed: Vec<&str> = launch.args.iter().map(String::as_str).collect();
    // As the Credential Store was derived from it, rather than as this command
    // spelled it: two spellings of one Profile are two keychain namespaces, and
    // the client would be pointed at the one Perch does not read.
    let told = probe::one_spelling(&profile);
    let ended = host.exec_interactive(
        &launch.program,
        &handed,
        &[("CLAUDE_CONFIG_DIR", &told.to_string_lossy())],
    );

    ended.map_err(|err| PerchError::Other(format!("could not launch {}: {err}", launch.said)))
}

/// The program a Run launches and what it is handed.
struct Launch {
    /// As the operating system will look for it: a path for the Claude Code the
    /// probe found, and otherwise the word as typed, so `npm` is found the way
    /// the shell would have found it.
    program: String,
    /// What it is handed, in the order it was typed. Claude Code's own arguments
    /// where nothing else was named, and the program's own where something was.
    args: Vec<String>,
    /// How the line printed before the launch says what is starting.
    said: String,
}

/// Reads the words after `--` as a command line.
///
/// The first word decides totally: a word beginning with `-` is an argument,
/// because nothing beginning with `-` can name a program the operating system
/// would find. Nothing after `--` at all is Claude Code with no arguments.
fn what_to_launch(host: &dyn Host, command: &[String]) -> Result<Launch> {
    match command.split_first() {
        // The empty string names a program the operating system would find no
        // more than a leading `-` does, and for the same reason: `PATH` is
        // searched for names and a path is written with a separator.
        Some((program, args)) if !program.is_empty() && !program.starts_with('-') => Ok(Launch {
            program: program.clone(),
            args: args.to_vec(),
            said: format!("`{program}`"),
        }),
        // The probe is what finds Claude Code, and it is reached only where
        // Claude Code is what is being launched: a Run of `npm` on a machine
        // Perch could not find a client on is still a Run of `npm`.
        _ => Ok(Launch {
            program: probe::claude_bin(host)?.to_string_lossy().into_owned(),
            args: command.to_vec(),
            said: "Claude Code".to_string(),
        }),
    }
}

/// Refuses `perch run <target> <anything>`, where what was meant for the program
/// was typed without the separator that says so. Read off the command line
/// before the parser sees it, because the parser is what the rule protects
/// against: clap claims `--resume` for Perch and reports an unknown argument. It
/// ends at the Target — anything before one is Perch's beyond doubt.
pub fn refuse_a_flag_without_the_separator(typed: &[String]) -> Result<()> {
    let typed = words(typed);
    let ["run", target, rest @ ..] = typed.as_slice() else {
        return Ok(());
    };
    if target.starts_with('-') {
        return Ok(());
    }

    // One word decides the whole line: what follows a Target is either the
    // separator or something that needed one. Nothing past `--` is read at all,
    // including a second `--`, which belongs to the program's own parser.
    let Some((word, _)) = rest.split_first() else {
        return Ok(());
    };
    if *word == "--" {
        return Ok(());
    }

    Err(PerchError::NotUnderstood(format!(
        "{} Everything meant for the program you are running goes after \
         `--`:\n\n    {}\n",
        whose(word),
        as_typed(target, rest)
    )))
}

/// Why the word was not Perch's to read, said in its own terms. A flag is
/// genuinely two things at once and the sentence says so; a bare word names a
/// program, so claiming an ambiguity there would be inventing one.
fn whose(word: &str) -> String {
    if word.starts_with('-') {
        format!("`{word}` could be Perch's flag or the program's, and Perch will not guess which.")
    } else {
        format!("`{word}` is a program to run rather than something Perch reads.")
    }
}

/// The command line as words to match against, which is all this rule reads it
/// as: everything about whose a flag is can be seen in the words themselves.
fn words(typed: &[String]) -> Vec<&str> {
    typed.iter().map(String::as_str).collect()
}

/// The line that would have worked, ready to be pasted back.
///
/// The words are shown as a shell would need them rather than as they arrived:
/// they reached this process with one layer of quoting already taken off, and a
/// suggestion that cannot be run is worse than no suggestion.
fn as_typed(target: &str, rest: &[&str]) -> String {
    let mut line = format!("perch run {} --", quoted_for_a_shell(target));
    for word in rest {
        line.push(' ');
        line.push_str(&quoted_for_a_shell(word));
    }
    line
}

/// One word as a shell would have to be given it, quoted only where it needs to
/// be so the common line reads as the person typed it.
fn quoted_for_a_shell(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || "-_=+.,:/@%^".contains(c));
    if plain {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// Refuses to launch a client against an Account whose Credential is known not
/// to work. It carries the Quarantine exit code: no amount of re-running repairs
/// it, and `perch relogin` does. Without this the user finds out from a Claude
/// Code that has already taken the terminal.
pub(crate) fn refuse_a_quarantined_account(registry: &Registry, email: &str) -> Result<()> {
    registry::refuse_a_quarantined_account(
        registry,
        email,
        "Nothing was launched. The client would open on an Account it cannot \
         authenticate as and ask you to log in.",
    )
}

/// What is about to happen, and what is not.
///
/// The second half is the whole point of the command: somebody who typed `run`
/// where they meant `switch` sees that nothing moved before the client takes the
/// screen.
fn launching(
    registry: &Registry,
    email: &str,
    said: &str,
    settled: Option<&registry::Settled>,
) -> String {
    let named = registry.named_for_the_user(email);
    // Nothing about who is active where nothing has settled who is active:
    // saying it about the Account a Switch was leaving is saying it about the
    // one Account it may no longer be true of.
    let active = settled.and_then(|settled| Some((settled, registry.active().whose()?)));
    match active {
        // Both Accounts named the way every other command names one, through
        // `is_active` — the one place the Registry answers a question about an
        // address, so an Alias `upsert` has respelled is not named twice.
        Some((settled, active)) if !registry.is_active(settled, email) => format!(
            "Running {said} as {named}, in this terminal alone. {} stays the \
             active Account everywhere else.",
            registry.named_for_the_user(active)
        ),
        // Running the Account that is already active is not a mistake worth
        // refusing: the Run still gets a Profile of its own, and the session it
        // launches is not the one a later Switch moves out from under.
        _ => format!("Running {said} as {named}, in this terminal alone."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EXIT_NOT_UNDERSTOOD;

    fn typed(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_string).collect()
    }

    fn refusal_for(line: &str) -> String {
        refuse_a_flag_without_the_separator(&typed(line))
            .expect_err("the line names a flag Perch will not claim")
            .to_string()
    }

    #[test]
    fn a_flag_after_the_target_is_refused_with_the_line_that_would_have_worked() {
        let said = refusal_for("run dev --resume");

        assert!(said.contains("`--resume`"), "{said}");
        assert!(said.contains("after `--`"), "{said}");
        assert!(said.contains("perch run dev -- --resume"), "{said}");
    }

    /// The suggestion is a line to paste back, and an Alias arrives here with
    /// one layer of quoting already taken off — so the target needs what every
    /// word after it gets. Unquoted, `it's` hangs a shell on an open quote and
    /// `my alias` re-refuses as two words.
    #[test]
    fn the_target_is_quoted_for_a_shell_like_every_other_word_on_the_line() {
        assert_eq!(as_typed("it's", &["-p"]), r"perch run 'it'\''s' -- -p");
        assert_eq!(as_typed("my alias", &["-p"]), "perch run 'my alias' -- -p");
        assert_eq!(as_typed("", &["-p"]), "perch run '' -- -p");
        assert_eq!(
            as_typed("dev", &["--resume"]),
            "perch run dev -- --resume",
            "and the ordinary target still reads as the person typed it"
        );
    }

    /// The exit code the argument parser itself would have used, so a script
    /// reads "that line was not a command" from one place.
    #[test]
    fn the_refusal_is_a_command_line_that_was_not_understood() {
        let refusal = refuse_a_flag_without_the_separator(&typed("run dev -p hello"))
            .expect_err("`-p` is a flag like any other");

        assert_eq!(refusal.exit_code(), EXIT_NOT_UNDERSTOOD);
    }

    #[test]
    fn the_suggested_line_carries_every_word_that_followed() {
        let said = refusal_for("run dev --resume --model opus -p hello");

        assert!(
            said.contains("perch run dev -- --resume --model opus -p hello"),
            "{said}"
        );
    }

    #[test]
    fn a_word_that_needs_quoting_is_quoted_in_the_suggestion() {
        let said = refuse_a_flag_without_the_separator(&[
            "run".to_string(),
            "dev".to_string(),
            "-p".to_string(),
            "two words".to_string(),
        ])
        .expect_err("`-p` is a flag")
        .to_string();

        assert!(said.contains("perch run dev -- -p 'two words'"), "{said}");
    }

    #[test]
    fn a_program_typed_without_the_separator_is_told_where_it_goes() {
        let said = refusal_for("run dev npm test");

        assert!(said.contains("`npm`"), "{said}");
        assert!(!said.contains("Perch's flag"), "{said}");
        assert!(said.contains("perch run dev -- npm test"), "{said}");
    }

    #[test]
    fn a_line_with_the_separator_is_left_alone() {
        for line in [
            "run dev -- --resume",
            "run dev -- npm test -- --watch",
            "run dev --",
            "run dev",
            "run",
        ] {
            assert!(
                refuse_a_flag_without_the_separator(&typed(line)).is_ok(),
                "{line}"
            );
        }
    }

    /// A suggestion built from a line with an unknown flag in front of the
    /// Target would drop that flag on the floor and read as though it had been
    /// accepted, so the parser keeps those lines.
    #[test]
    fn a_flag_before_the_target_is_the_parsers_business() {
        for line in ["run --help", "run -h", "run --json dev --resume"] {
            assert!(
                refuse_a_flag_without_the_separator(&typed(line)).is_ok(),
                "{line}"
            );
        }
    }

    #[test]
    fn no_other_command_is_touched() {
        for line in ["list --json", "list work --refresh", "add --no-group"] {
            assert!(
                refuse_a_flag_without_the_separator(&typed(line)).is_ok(),
                "{line}"
            );
        }
    }
}
