//! `perch upgrade` — this machine's Installation, replaced with a newer Release
//! (ADR 0039).
//!
//! **It routes rather than overwrites.** Three of the four Channels manage the
//! binary themselves, and writing over one of theirs is not a shortcut but a
//! corruption: a Homebrew Cellar whose receipt no longer describes what is in
//! it reverts at the next `brew upgrade`, and an npm platform package
//! overwritten in place leaves the wrapper depending on a version that is no
//! longer there. So Perch works out which Channel left this Installation and
//! hands the work back to it, and replaces the binary itself only for the one
//! Channel that leaves nothing else in charge.
//!
//! **It touches nothing Perch holds.** No registry, no lock, no Credential —
//! an Installation is the counterpart to what Perch holds, not part of it, so
//! this command runs the same on a machine with four Accounts and on one with
//! none.

use std::io::Write;

use crate::commands::{ask_a_word, say, say_json};
use crate::error::{PerchError, Result};
use crate::host::{Host, Platform};
use crate::upgrade::{self, Channel};

#[derive(Debug, Clone, Default)]
pub struct UpgradeArgs {
    /// The Release to install. Without it, the newest.
    pub release: Option<String>,
    /// Say what is installed and what is newest, and do nothing.
    pub check: bool,
    /// That answer as a document, for something parsing it.
    pub json: bool,
    /// The Channel this Installation came from, for when the path does not say.
    pub channel: Option<String>,
    /// Agreement given ahead of time to a Release older than the one installed.
    pub yes: bool,
}

pub fn run(host: &dyn Host, args: UpgradeArgs, out: &mut dyn Write) -> Result<i32> {
    // A check is asked before the Channel is insisted on, because a check makes
    // no use of one it cannot answer without. `chosen_channel` refuses a binary
    // nothing placed — the right answer for an Upgrade, which is about to write
    // over that binary, and the wrong one for a question that writes nothing: a
    // hand-unpacked Perch got "Perch will not write over a file it did not put
    // there" from `--check --json`, a sentence about an act it had not asked
    // for, and no `installed` or `newest` at all. Answering it is success
    // whichever way the answer went (ADR 0039).
    if args.check {
        let channel = chosen_channel(host, args.channel.as_deref()).ok();
        return check(host, channel.as_ref(), args.json, out).map(|()| crate::error::EXIT_OK);
    }

    let channel = chosen_channel(host, args.channel.as_deref())?;
    if args.json {
        return Err(PerchError::Invalid(
            "`--json` says what a check found, so it goes with `--check`.\n\
             `perch upgrade --check --json` is the line that answers."
                .to_string(),
        ));
    }

    // Before anything is resolved and before anybody is asked to agree to
    // anything. A Release Homebrew cannot be pointed at is refused whatever the
    // Release turns out to be, so asking first put the question the wrong way
    // round: `--release 0.0.1` on a newer build showed "Install the older
    // Release? [y/N]", waited for a `y`, and only then said Homebrew could not
    // take one — and with no terminal it advised `--yes`, which reaches the same
    // refusal. A `brew` that is not on PATH answered earlier still, naming the
    // wrong problem entirely.
    if matches!(channel, Channel::Homebrew { .. }) {
        refuse_a_release_homebrew_cannot_take(&args.release)?;
    }

    let wanted = match &args.release {
        Some(typed) => upgrade::version_typed(typed)?,
        None => upgrade::newest(host, None)?,
    };
    let installed = upgrade::installed();

    match upgrade::compare(&wanted, installed) {
        std::cmp::Ordering::Equal => {
            return Err(PerchError::NothingToDo(format!(
                "{installed} is already what is installed, and it came from \
                 {}.\n\
                 `perch upgrade --check` says what the newest Release is.",
                channel.name()
            )));
        }
        std::cmp::Ordering::Less => agree_to_going_back(host, &wanted, installed, args.yes, out)?,
        std::cmp::Ordering::Greater => {}
    }

    match &channel {
        Channel::Homebrew { prefix } => {
            let (brew, brew_args) = upgrade::homebrew_command(host, prefix)?;
            hand_it_over(host, &brew, &brew_args, out)
        }
        Channel::Npm => {
            // The version as Perch reads it rather than as it was typed: npm
            // has never had the leading `v`, so `--release v0.2.0` reaching it
            // verbatim is a package nobody published.
            let named = args.release.as_ref().map(|_| wanted.as_str());
            let (npm, npm_args) = upgrade::npm_command(host, named)?;
            // npm would be replacing `perch.exe` while it is the running
            // process, and Windows holds a running executable open. So the
            // command is printed rather than run: it works perfectly well from
            // a shell where Perch is not running, and not at all from here.
            if host.platform() == Platform::Windows {
                say(
                    out,
                    &format!(
                        "This Installation came from npm, and npm cannot replace \
                         `perch.exe` while it is running.\n\
                         Run this from a terminal where Perch is not running:\n\
                         \n    {}\n",
                        upgrade::as_typed(&npm, &npm_args)
                    ),
                )?;
                return Ok(crate::error::EXIT_OK);
            }
            hand_it_over(host, &npm, &npm_args, out)
        }
        Channel::Installer => replace_it_ourselves(host, &wanted, out),
    }
}

/// The Channel a person named, or the one the path says, or a refusal.
///
/// A named Channel is taken as given and not checked against the path: the
/// whole of what `--channel` is for is the machine where the path is wrong —
/// somebody who symlinked the binary, or relocated it, or is running one out of
/// a directory Perch has never heard of.
fn chosen_channel(host: &dyn Host, named: Option<&str>) -> Result<Channel> {
    if let Some(word) = named {
        return Channel::spelled(word).ok_or_else(|| {
            PerchError::Invalid(format!(
                "`{word}` is not a Channel. They are `homebrew`, `npm` and \
                 `installer`."
            ))
        });
    }

    let exe = host
        .current_exe()
        .map_err(|err| PerchError::Other(format!("could not find Perch's own binary: {err}")))?;

    upgrade::channel(host)?.ok_or_else(|| {
        // The installer's directory as *this* machine would have it, rather
        // than the Unix default written out: on Windows that is somewhere else
        // entirely, and naming a path the reader does not have is how a refusal
        // stops being actionable.
        let expected = upgrade::installer_dir(host)
            .map(|dir| dir.display().to_string())
            .unwrap_or_else(|_| "its own directory".to_string());

        PerchError::Invalid(format!(
            "Perch is installed at {}, and nothing about that path says which \
             Channel put it there.\n\
             Homebrew and npm keep their binaries somewhere Perch recognises, \
             and the installer script puts one in {expected}. A binary anywhere \
             else was placed by hand — most likely unpacked from the Release \
             page — and Perch will not write over a file it did not put there.\n\
             Re-run the installer from https://github.com/{} to move to a \
             managed Installation, or say which Channel this is with \
             `--channel homebrew|npm|installer`.",
            exe.display(),
            upgrade::REPO
        ))
    })
}

/// What is installed, what is newest, and where this one came from.
///
/// Exits nought either way. A check is a question, and answering it is success
/// whichever way the answer went — every other non-zero code Perch has is a
/// refusal, and "there is news" is not one (ADR 0039).
///
/// The Channel is optional here alone. An Upgrade cannot proceed without one,
/// because it is about to write over a binary and has to know whose it is; a
/// check only reports it, and the two facts a script came for — what is
/// installed and what is newest — are knowable whether or not the path says who
/// put it there. `null` is the honest answer for that rather than a refusal.
fn check(
    host: &dyn Host,
    channel: Option<&Channel>,
    json: bool,
    out: &mut dyn Write,
) -> Result<()> {
    let installed = upgrade::installed();
    let newest = upgrade::newest(host, None)?;
    let behind = upgrade::compare(&newest, installed) == std::cmp::Ordering::Greater;

    if json {
        return say_json(
            out,
            &serde_json::json!({
                "installed": installed,
                "newest": newest,
                "channel": channel.map(Channel::word),
                "upgradeAvailable": behind,
            }),
        );
    }

    say(out, &format!("installed  {installed}"))?;
    say(out, &format!("newest     {newest}"))?;
    say(
        out,
        &format!(
            "channel    {}",
            channel.map_or("unknown (nothing about this binary's path says)", |channel| {
                channel.name()
            })
        ),
    )?;
    say(
        out,
        match (behind, channel.is_some()) {
            // Said only where it is true. On a binary nothing placed, `perch
            // upgrade` refuses rather than taking anything, and a check that
            // pointed at it would be sending somebody to the refusal it was
            // just careful not to give them.
            (true, true) => "\nA newer Release is available. `perch upgrade` takes it.",
            (true, false) => {
                "\nA newer Release is available. This Perch was placed by hand, so \
                 `perch upgrade` needs `--channel homebrew|npm|installer` to say \
                 which Channel should replace it."
            }
            (false, _) => "\nNothing newer has been published.",
        },
    )
}

/// Agreement to install a Release older than the one running.
///
/// Named as a downgrade and told what it costs, rather than asked as a bare
/// "are you sure" — because the cost is specific and is not obvious: Perch
/// refuses a registry written by a newer Perch, so going back far enough leaves
/// a working machine with a binary that will not read its own state.
fn agree_to_going_back(
    host: &dyn Host,
    wanted: &str,
    installed: &str,
    yes: bool,
    out: &mut dyn Write,
) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !host.is_interactive() {
        return Err(PerchError::Invalid(format!(
            "{wanted} is older than the {installed} that is installed, and \
             there is no terminal to agree to that on.\n\
             A Perch older than the one that last wrote your registry refuses \
             to read it. `--yes` says you have accounted for that."
        )));
    }

    say(
        out,
        &format!(
            "{wanted} is older than the {installed} that is installed.\n\
             Perch refuses a registry written by a newer Perch, so if {installed} \
             has written yours, {wanted} will not read it — `perch upgrade` \
             back to {installed} is the repair."
        ),
    )?;
    match ask_a_word(host, out, "Install the older Release? [y/N] ")?.as_deref() {
        Some("y") | Some("yes") => Ok(()),
        _ => Err(PerchError::NothingToDo(
            "Nothing was installed.".to_string(),
        )),
    }
}

/// Homebrew installs what the formula says and has no way to be pointed at an
/// older Release, so `--release` there is a request that cannot be honoured.
///
/// Refused rather than quietly ignored: silently installing the newest when
/// somebody named an older one is the failure mode where they find out by
/// reading `perch --version` afterwards, if they think to.
fn refuse_a_release_homebrew_cannot_take(release: &Option<String>) -> Result<()> {
    match release {
        None => Ok(()),
        Some(named) => Err(PerchError::Invalid(format!(
            "This Installation came from Homebrew, which installs whatever the \
             formula names and cannot be pointed at {named}.\n\
             `brew upgrade perch` takes the newest. To hold a particular \
             Release, install it with the installer script instead — it takes \
             `PERCH_VERSION`."
        ))),
    }
}

/// Runs the Channel's own command, having said what it is.
///
/// Said first, because a command that hands the terminal to `brew` for two
/// minutes without saying so reads as a hang — and because somebody who would
/// rather run it themselves can stop here and do that. The terminal goes with
/// it: what `brew` and `npm` print is progress, not something Perch reads.
///
/// Their exit status is Perch's. A failed `brew upgrade` is a failed upgrade,
/// and translating it into a code of Perch's own would lose which of `brew`'s
/// many failures it was.
fn hand_it_over(
    host: &dyn Host,
    program: &std::path::Path,
    args: &[String],
    out: &mut dyn Write,
) -> Result<i32> {
    say(out, &upgrade::as_typed(program, args))?;
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    host.exec_interactive(&program.to_string_lossy(), &borrowed, &[])
        .map_err(|err| {
            PerchError::Other(format!(
                "could not run `{}`: {err}",
                upgrade::as_typed(program, args)
            ))
        })
}

/// The one Channel Perch replaces itself for, done by the installer that made
/// the Installation in the first place.
///
/// The script is embedded rather than fetched (ADR 0039) and written where
/// Perch keeps its own things, closed to everyone but its owner: it is a
/// program about to be run, and one a second user on the machine could edit
/// between the write and the run is a program somebody else chose.
///
/// No execute bit, because it is never executed by name — `sh` and `powershell`
/// are handed it as a file to read, which is also what keeps this working on a
/// `noexec` mount.
fn replace_it_ourselves(host: &dyn Host, wanted: &str, out: &mut dyn Write) -> Result<i32> {
    let (name, script) = upgrade::installer_for(host.platform());
    // Spelled with `/` for the reason `upgrade::beneath` is written down at:
    // what this path is handed to follows the platform the Host reports, and
    // `Path::join` would follow the build instead.
    let at = std::path::PathBuf::from(format!(
        "{}/{name}",
        crate::registry::perch_home(host)?.display()
    ));

    host.write_private_file(&at, script)
        .map_err(|err| PerchError::Other(format!("could not write {}: {err}", at.display())))?;

    let tag = upgrade::tag_of(wanted);
    say(out, &format!("installing {tag} over this Installation"))?;

    let ran = run_the_installer(host, &at, &tag);

    // Cleared whichever way it went. What is left behind otherwise is an
    // executable script in Perch's own directory that nothing will ever run
    // again, and that a later Perch would have no way to tell from one it was
    // about to use.
    let _ = host.remove_file(&at);

    ran
}

fn run_the_installer(host: &dyn Host, at: &std::path::Path, tag: &str) -> Result<i32> {
    let script = at.to_string_lossy().into_owned();
    // The terminal goes to the installer: what it prints is which checks it
    // made, and those are for the person to read rather than for Perch to
    // parse.
    let (program, args) = match host.platform() {
        Platform::Windows => (
            "powershell".to_string(),
            vec![
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-File".to_string(),
                script,
            ],
        ),
        _ => ("/bin/sh".to_string(), vec![script]),
    };

    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    host.exec_interactive(&program, &borrowed, &[("PERCH_VERSION", tag)])
        .map_err(|err| PerchError::Other(format!("could not run the installer: {err}")))
}
