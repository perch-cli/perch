//! One process uses one Account's Profile (ADR a-run-is-one-shot).
//!
//! Runs stay pinned while provider Defaults change. Claude Profiles reconcile
//! shared state; Codex Profiles isolate auth, configuration, and history.
//! A Marker protects the Profile while the child lives; no Registry lock spans
//! the session. Perch's remarks go to stderr so stdout belongs to the child.

use std::io::Write;

use crate::adopt;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{self, Registry};
use crate::target;

/// What a Run was asked for. `command` is text, like every other word Perch
/// takes: one that is not text is refused by the parser rather than mangled here.
#[derive(Debug, Clone, clap::Args)]
pub struct RunArgs {
    #[command(flatten)]
    pub provider: super::selection::Selection,
    /// An Alias or email address
    pub target: String,

    /// After `--`, a program and its arguments, or the provider's own
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
    let custom = args
        .command
        .first()
        .is_some_and(|word| !word.is_empty() && !word.starts_with('-'));
    let installation = if custom {
        None
    } else if !registry.run_fallback && args.provider.explicit()?.is_none() {
        Some(
            registry
                .run_provider
                .adapter()
                .configured(host)?
                .installation(host)?,
        )
    } else {
        Some(args.provider.installed(host, registry.run_provider)?)
    };
    let selected = installation
        .as_ref()
        .map(|installed| installed.provider())
        .or(args.provider.explicit()?);
    let found = target::resolve_for(&registry, &args.target, selected)?;
    let account = registry.held(&found.email)?;
    if selected.is_some_and(|provider| provider != account.provider()) {
        return Err(PerchError::Invalid(format!(
            "{} is a {} Account. `perch run --{} {}` launches it.",
            args.target,
            account.provider().adapter().name(),
            account.provider().word(),
            args.target
        )));
    }
    refuse_a_quarantined_account(&registry, account.key())?;
    let held = crate::holdings::lock(host)?;
    let latest = registry::load(host)?.ok_or_else(|| {
        PerchError::NotFound("The configuration disappeared before launch".into())
    })?;
    let current = latest.held(account.key())?;
    if current.provider() != account.provider()
        || current.provider_identity != account.provider_identity
        || current.identity.account_uuid != account.identity.account_uuid
        || current.identity.organization_uuid != account.identity.organization_uuid
    {
        return Err(PerchError::Invalid(
            "The Account identity changed before launch; run the command again. Nothing was launched."
                .into(),
        ));
    }
    let account = current;
    refuse_a_quarantined_account(&latest, account.key())?;
    crate::switch::refuse_a_shared_profile(account, &latest)?;
    let active = latest.active_for(account.provider());
    let mut shared_profiles = Vec::new();
    if account.provider().adapter().capabilities().shared_state
        && !matches!(active, crate::registry::Active::Landing { .. })
    {
        for peer in &latest.accounts {
            let same_group = match (&peer.group, &account.group) {
                (Some(a), Some(b)) => crate::name::same_name(a, b),
                _ => false,
            };
            if peer.provider() == account.provider() && (peer.key() == account.key() || same_group)
            {
                shared_profiles.push(crate::providers::provider::SharedProfile {
                    path: peer.profile_dir(host)?,
                    is_default: active.is_active(peer.key()),
                });
            }
        }
    }
    let profile = account.profile(host)?;
    let launch = account.provider().adapter().prepare_launch(
        host,
        &crate::providers::provider::LaunchRequest {
            kind: match &installation {
                Some(installed) => crate::providers::provider::LaunchKind::Client(installed),
                None => crate::providers::provider::LaunchKind::Custom(&args.command[0]),
            },
            account: &profile,
            arguments: if custom {
                &args.command[1..]
            } else {
                &args.command
            },
            shared_profiles,
        },
    )?;
    drop(held);
    let program = if custom {
        format!("`{}`", launch.program())
    } else {
        account.provider().adapter().name().to_string()
    };
    host.note(&format!(
        "Running {program} as {}, in this terminal alone.",
        latest.named_for_the_user(account.key())
    ));
    out.flush().map_err(crate::say::failed)?;
    launch.execute(host)
}

/// Refuses `perch run <target> <anything>`, where what was meant for the program
/// was typed without the separator that says so. Read off the command line
/// before the parser sees it, because the parser is what the rule protects
/// against: clap claims `--resume` for Perch and reports an unknown argument. It
/// ends at the Target — anything before one is Perch's beyond doubt.
pub fn refuse_a_flag_without_the_separator(typed: &[String]) -> Result<()> {
    let mut filtered = Vec::new();
    let mut input = words(typed).into_iter();
    while let Some(word) = input.next() {
        if word == "--" {
            filtered.push(word);
            filtered.extend(input);
            break;
        }
        if matches!(word, "--claude" | "--codex") || word.starts_with("--provider=") {
            continue;
        }
        if word == "--provider" {
            input.next();
            continue;
        }
        filtered.push(word);
    }
    let typed = filtered;
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
