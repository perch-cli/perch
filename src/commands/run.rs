//! `perch run <target>` — a client against one Account's Profile, without a
//! Switch (ADR 0010).
//!
//! One process is pointed at one Profile by setting `CLAUDE_CONFIG_DIR` for it
//! and nothing else. The active Account, every other terminal, the editor
//! extension and the desktop app go on as they were: nothing is Captured,
//! nothing is written to the Default Profile, and no Identity is patched. A Run
//! is not a Switch and shares none of its machinery.
//!
//! Several Runs coexist. Two terminals running two Accounts is what the command
//! is for rather than an edge case, so nothing here may hold anything for as
//! long as the client lives — the registry least of all, which is read and let
//! go before the launch.
//!
//! A Run is the one path where a Profile is a live configuration directory
//! rather than storage, so it is the one path that has to Reconcile
//! ([`crate::reconcile`]) — and it does so before every launch, because Shared
//! State moves underneath it between Runs. A Run that cannot Reconcile does not
//! launch: a client served a Profile it cannot see the person's memory,
//! settings and plugins through is worse than one that did not start.
//!
//! It is also the one path that makes a Profile **Live** (ADR 0027). A Run
//! writes the session marker that says so and takes it away when the Run ends,
//! which is what stops another Perch Capturing, Renewing or writing
//! `.claude.json` into a Profile somebody is working in. Reading out of one is
//! untouched: a Switch onto the Account a Run is against still lands.
//!
//! Everything Perch says about a Run goes to standard error, which every other
//! remark about the machine already does. The client inherits this process's
//! standard output, and a Run is meant to stand in a script wherever `claude`
//! would — so two lines of Perch's own in front of `claude -p … --output-format
//! json` is a document that no longer parses. What the client says on stdout is
//! the whole of what a Run says on stdout.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::adopt;
use crate::commands;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{self, Registry};
use crate::{carry, probe, reconcile, target};

#[derive(Debug, Clone)]
pub struct RunArgs {
    /// The Account to run as: its Alias, or its email address.
    pub target: String,

    /// What was typed after `--`, one word per element and none of them read:
    /// the program to run and its arguments, or Claude Code's own arguments
    /// where the first word is a flag. Empty is `perch run <target>` on its own.
    ///
    /// Text, like every other word Perch takes. A word that is not text is
    /// refused by the parser rather than mangled here, which is the honest
    /// answer until somebody has one to forward — and forwarding it would mean
    /// an `OsString` in every hand it passes through.
    pub command: Vec<String>,
}

/// Launches a client against the named Account's Profile and reports the status
/// it exited with.
///
/// The status comes back rather than being folded into success or failure: a
/// Run is a way of launching a program, and a wrapper that flattened what the
/// program said would break every script that branches on it.
pub fn run(host: &dyn Host, args: RunArgs, out: &mut dyn Write) -> Result<i32> {
    // Read and let go, never held. A Run lasts as long as somebody's session,
    // and a registry lock held across that would shut every other Perch on the
    // machine out for the afternoon — including the second Run this command
    // exists to make possible.
    let registry = adopt::ensure_adopted(host)?;

    // A Group names a set of Accounts declared interchangeable, which is what a
    // Cycle needs and nothing a Run can act on: there is no one Profile to point
    // a process at. Refused here, in the one place every command that acts on
    // exactly one Account refuses it.
    let found = target::resolve_account(&registry, &args.target)?;
    host.note(&found.matched);
    refuse_a_quarantined_account(&registry, &found.email)?;

    // Settled before anything is linked. Where this is the Claude Code the probe
    // has to find, a machine without one is a refusal that should cost the
    // filesystem nothing — and the launch is the only thing Reconcile is
    // preparation for either way.
    let launch = what_to_launch(host, &args.command)?;
    let profile = registry::profile_dir_for(host, &found.email)?;
    let default_profile = registry::the_default_profile(host)?;
    reconcile::reconcile(host, &default_profile.config_dir, &profile)?;

    // The one file Reconcile cannot link, because it holds the Account as well
    // as the person (ADR 0003). Handled key by key, and afterwards: Reconcile is
    // what makes the Profile a directory at all.
    carry::carry(host, &registry, &found.email, &default_profile, &profile);

    host.note(&launching(&registry, &found.email, &launch.said));
    // Flushed before the client is handed the terminal. Nothing of Perch's own
    // goes to standard output here, but a command run before this one may have
    // left something in the buffer, and it would be delivered after the output
    // of the thing it was announcing.
    out.flush().map_err(commands::write_failed)?;

    // After the Carry, which will not write into a Live Profile and would
    // therefore refuse its own Run; and immediately before the launch, with
    // nothing fallible in between, so the marker cannot outlive a Run that never
    // started (ADR 0027).
    let marker = mark_live(host, &profile)?;

    // The environment of this one process, and the whole of what makes the Run
    // a Run.
    let handed: Vec<&str> = launch.args.iter().map(String::as_str).collect();
    let ended = host.exec_interactive(
        &launch.program,
        &handed,
        &[("CLAUDE_CONFIG_DIR", &profile.to_string_lossy())],
    );

    // However it ended, including not having started. The Profile stops being
    // Live when the Run does.
    let _ = host.remove_file(&marker);

    ended.map_err(|err| PerchError::Other(format!("could not launch {}: {err}", launch.said)))
}

/// Makes the Profile a Live Profile for the length of the Run, and hands back
/// the marker that says so.
///
/// A Profile with a Run against it is Live, evidenced the way ADR 0022 already
/// evidences one: a session marker naming a process that is still the one that
/// wrote it. The process named is Perch's own, because Perch waits for what it
/// launched — so the marker holds for exactly as long as the Run, and it can be
/// written *before* the launch, which the child's pid cannot.
///
/// Refused rather than remarked on when it cannot be written. What the marker
/// buys is that no other Perch Captures, Renews or writes `.claude.json` into
/// this Profile while somebody is working in it — a mid-task logout, silently,
/// which is the failure ADR 0005 exists for. A Run that cannot claim its Profile
/// is a Run nothing is protecting, and a person told so beforehand has lost a
/// command rather than their work.
///
/// The failure is a refused Run rather than a bare `file_write`, which is how
/// [`crate::reconcile`] reports the same class of thing on this same path: the
/// person needs to know that nothing was launched and why, and a line naming
/// only the file leaves them to work out both.
fn mark_live(host: &dyn Host, profile: &Path) -> Result<PathBuf> {
    let pid = host.process_id();
    let marker = probe::session_marker_at(profile, pid);

    // Atomically, so a reader sees the whole marker or no marker at all. A
    // plain write truncates and then fills, and a reader catching it in between
    // reads a file it can see all of and that says nothing — which `clients_in`
    // settles as "not Live", the opposite of the arm its comment says this case
    // takes. A `perch switch` in that window Captures the Credential the Run is
    // about to hand a client (ADR 0027).
    host.create_dir_all(&probe::sessions_dir(profile))
        .and_then(|()| {
            crate::host::write_atomically(host, &marker, &probe::session_marker(pid, host.now()))
        })
        .map_err(|err| {
            PerchError::Other(format!(
                "{} could not be written ({err}), so Perch cannot record that a \
                 client is running against this Profile — and another Perch \
                 would be free to Capture or Renew the Credential that client \
                 is holding. Nothing was launched.",
                marker.display()
            ))
        })?;

    Ok(marker)
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

/// Reads the words after `--` as a command line (ADR 0010).
///
/// The first word decides, and decides totally: a word beginning with `-` is an
/// argument, because nothing that begins with `-` can name a program the
/// operating system would find — `PATH` is searched for names and a path is
/// written with a `/`, and somebody with a file called `-resume` reaches it as
/// `./-resume`. So `perch run dev -- --resume` is Claude Code resuming and
/// `perch run dev -- npm test` is `npm`, with nothing guessed in either case.
///
/// Nothing after `--`, and nothing after the Target at all, is Claude Code with
/// no arguments: the command's first and commonest form.
fn what_to_launch(host: &dyn Host, command: &[String]) -> Result<Launch> {
    match command.split_first() {
        // The empty string names a program the operating system would find no
        // more than a leading `-` does, and for the same reason: `PATH` is
        // searched for names and a path is written with a separator. Without
        // this, `perch run dev -- '' --resume` announced "Running `` as …" and
        // handed `""` to the operating system to launch.
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
/// was typed without the separator that says so (ADR 0010).
///
/// Read off the command line before the parser sees it, because the parser is
/// what the rule exists to protect against: clap will claim `--resume` for Perch
/// and report an unknown argument, which is true and is not the thing the person
/// needs to be told. Both readings of that line are real — Perch has a `--json`
/// and so does Claude Code — so the answer is neither reading, and the message
/// is the line that would have worked.
///
/// A word that is not a flag is caught by the same rule and told the same thing,
/// because `perch run dev npm test` is the same mistake made without a dash:
/// nothing but `--` follows a Target, so there is no reading of it to argue
/// about, only a separator to put in.
///
/// It ends at the Target, and only where the Target is the first word after
/// `run`. Anything before it is the parser's: a flag there is Perch's beyond
/// doubt — `perch run --help` is a request for help with `run`, and no program
/// has been named yet for it to have belonged to — and a line the parser will
/// reject anyway should be rejected in its words rather than answered here with
/// a suggestion that quietly drops half of what was typed.
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

/// Why the word was not Perch's to read, said in its own terms.
///
/// A flag is genuinely two things at once and the sentence says so. A bare word
/// never was — it names a program — so telling somebody Perch could not tell
/// whose it was would be inventing an ambiguity to explain.
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
    let mut line = format!("perch run {target} --");
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
/// to work.
///
/// The remedy is the one every Quarantine has and no other refusal in Perch has,
/// so it carries the same exit code: no amount of re-running repairs it, and
/// `perch relogin` does. Without this the user finds out by being asked to log
/// in by a Claude Code that has already taken the terminal.
///
/// Reached by the TUI as well, which refuses the keystroke while it still has
/// the terminal: a Run from the picker gives the screen back before it launches
/// anything, and a refusal arriving after that is one the user reads with the
/// view they were choosing in already gone.
pub(crate) fn refuse_a_quarantined_account(registry: &Registry, email: &str) -> Result<()> {
    crate::commands::refuse_a_quarantined_account(
        registry,
        email,
        "Nothing was launched — the client would open on an Account it cannot \
         authenticate as and ask you to log in.",
    )
}

/// What is about to happen, and what is not.
///
/// The second half is the whole point of the command, and the difference from
/// the command next to it: somebody who typed `run` where they meant `switch`
/// should be able to see that nothing moved before the client takes the screen.
fn launching(registry: &Registry, email: &str, said: &str) -> String {
    let named = registry.named_for_the_user(email);
    match registry.active.as_deref() {
        // Both Accounts are named the way every other command names one, so the
        // sentence that contrasts them does not hand one of them its Alias and
        // take the other's away.
        Some(active) if active != email => format!(
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

    /// The exit code the argument parser itself would have used, so a script
    /// reads "that line was not a command" from one place.
    #[test]
    fn the_refusal_is_a_command_line_that_was_not_understood() {
        let refusal = refuse_a_flag_without_the_separator(&typed("run dev -p hello"))
            .expect_err("`-p` is a flag like any other");

        assert_eq!(refusal.exit_code(), EXIT_NOT_UNDERSTOOD);
    }

    /// Everything the person typed comes back in the suggestion, in the order
    /// they typed it: a corrected line missing half the command is a second
    /// thing for them to get right.
    #[test]
    fn the_suggested_line_carries_every_word_that_followed() {
        let said = refusal_for("run dev --resume --model opus -p hello");

        assert!(
            said.contains("perch run dev -- --resume --model opus -p hello"),
            "{said}"
        );
    }

    /// The suggestion is meant to be pasted, and a word with a space in it that
    /// came off the command line as one word has to go back as one word.
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

    /// The same mistake without a dash on it. Nothing but `--` may follow a
    /// Target, so a program typed straight after one is told where it goes
    /// rather than left to the parser to call an unexpected argument.
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

    /// A flag before the Target belongs to Perch beyond doubt — there is no
    /// program named yet for it to have been meant for — so the parser answers
    /// for it, and answers with help rather than with a refusal. The parser also
    /// keeps the lines it would reject anyway: a suggestion built from a line
    /// with an unknown flag in front of the Target would drop that flag on the
    /// floor and read as though it had been accepted.
    #[test]
    fn a_flag_before_the_target_is_the_parsers_business() {
        for line in ["run --help", "run -h", "run --json dev --resume"] {
            assert!(
                refuse_a_flag_without_the_separator(&typed(line)).is_ok(),
                "{line}"
            );
        }
    }

    /// Every other command has flags of its own and none of them launches
    /// anything, so the rule is `run`'s alone.
    #[test]
    fn no_other_command_is_touched() {
        for line in ["list --json", "status --group --refresh", "add --no-group"] {
            assert!(
                refuse_a_flag_without_the_separator(&typed(line)).is_ok(),
                "{line}"
            );
        }
    }
}
