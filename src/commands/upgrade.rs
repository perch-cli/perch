//! `perch upgrade` — this machine's Installation, replaced with a newer Release
//! (ADR an-upgrade-asks-its-channel).
//!
//! **It routes rather than overwrites.** Three of the four Channels manage the
//! binary themselves, so writing over one is a corruption rather than a
//! shortcut: Perch hands the work back to the Channel that left this
//! Installation, and replaces the binary only for the one that leaves nobody.
//!
//! It writes no registry, no lock and no Credential — only, on the installer
//! Channel, a script under Perch's home, which on a fresh machine makes it.

use std::io::Write;

use crate::commands::{Presumed, said_yes, say, say_json};
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
    // Before the Channel is insisted on: `chosen_channel` refuses a binary
    // nothing placed, which is the right answer for a command about to write over
    // it and the wrong one for a question that writes nothing.
    if args.check {
        // Only the *path's* answer is allowed to be missing. A word somebody
        // typed is a word, and `homebre` is a typo rather than a machine nothing
        // placed.
        let named = args.channel.as_deref().map(named_channel).transpose()?;
        let channel = match named {
            Some(channel) => Some(channel),
            None => upgrade::channel(host).unwrap_or_default(),
        };
        return check(host, channel.as_ref(), args.json, out).map(|()| crate::error::EXIT_OK);
    }

    let channel = chosen_channel(host, args.channel.as_deref())?;

    // Before anything is resolved and before anybody is asked to agree to
    // anything: this refusal holds whatever the Release turns out to be, and
    // nobody should agree to something that is refused either way.
    if matches!(channel, Channel::Homebrew { .. }) {
        refuse_a_release_homebrew_cannot_take(&args.release)?;
    }
    if matches!(channel, Channel::Npm) {
        refuse_npm_replacing_a_running_perch(host, &args.release)?;
    }

    let wanted = match &args.release {
        Some(typed) => Some(upgrade::version_typed(typed)?),
        None => newest_or_let_the_channel_say(host, &channel)?,
    };
    let installed = upgrade::installed();

    if let Some(wanted) = &wanted {
        match upgrade::compare(wanted, installed) {
            std::cmp::Ordering::Equal => {
                return Err(PerchError::NothingToDo(format!(
                    "{installed} is already what is installed, and it came from \
                     {}.\n\
                     `perch upgrade --check` says what the newest Release is.",
                    channel.name()
                )));
            }
            std::cmp::Ordering::Less => {
                agree_to_going_back(host, wanted, installed, args.yes, out)?;
            }
            std::cmp::Ordering::Greater => {}
        }
    }

    let replaced = match &channel {
        Channel::Homebrew { prefix } => {
            let (brew, brew_args) = upgrade::homebrew_command(host, prefix)?;
            hand_it_over(host, &brew, &brew_args, out)
        }
        Channel::Npm => {
            // The version as Perch reads it rather than as typed: npm has never
            // had the leading `v`, so `v0.2.0` is a package nobody published.
            let named = args.release.as_ref().and(wanted.as_deref());
            let (npm, npm_args) = upgrade::npm_command(host, named)?;
            hand_it_over(host, &npm, &npm_args, out)
        }
        // The one Channel with nothing to hand the work to, so its Release is
        // the answer `newest_or_let_the_channel_say` never goes without.
        Channel::Installer => {
            replace_it_ourselves(host, wanted.as_deref().unwrap_or_default(), out)
        }
    }?;

    // The Channel moved the binary and neither `brew` nor `npm` has heard of a
    // unit file (ADR the-machine-runs-the-watcher), so it is written again — said
    // rather than raised, and only where the Channel's own command succeeded.
    if replaced == crate::error::EXIT_OK
        && let Some(said) = crate::commands::service::refreshed_after_an_upgrade(host)
    {
        say(out, &said)?;
    }
    Ok(replaced)
}

/// Which Release to install, or `None` where the Channel works that out itself
/// and could not be told. On those two the answer decides nothing but whether
/// Perch says "already the newest", and unauthenticated `api.github.com` allows
/// 60 requests an hour per address — which a shared one reaches.
fn newest_or_let_the_channel_say(host: &dyn Host, channel: &Channel) -> Result<Option<String>> {
    match upgrade::newest(host, None) {
        Ok(newest) => Ok(Some(newest)),
        Err(unreachable) if channel.resolves_its_own() => {
            host.note(&format!(
                "{unreachable}\n\nSo Perch cannot say whether there is anything \
                 newer. Handing the work to {} regardless, which works it out \
                 for itself.",
                channel.name(),
            ));
            Ok(None)
        }
        Err(unreachable) => Err(unreachable),
    }
}

/// The Channel a word names, or a refusal naming the three there are.
///
/// Apart from [`chosen_channel`] because a check needs this half without the
/// other: what it may go without is the answer read off the *path*.
fn named_channel(word: &str) -> Result<Channel> {
    Channel::spelled(word).ok_or_else(|| {
        PerchError::Invalid(format!(
            "`{word}` is not a Channel. They are `homebrew`, `npm` and \
             `installer`."
        ))
    })
}

/// The Channel a person named, or the one the path says, or a refusal.
///
/// A named Channel is taken as given rather than checked against the path: what
/// `--channel` is for is the machine where the path is wrong.
fn chosen_channel(host: &dyn Host, named: Option<&str>) -> Result<Channel> {
    if let Some(word) = named {
        return named_channel(word);
    }

    let exe = host
        .current_exe()
        .map_err(|err| PerchError::Other(format!("could not find Perch's own binary: {err}")))?;

    upgrade::channel(host)?.ok_or_else(|| {
        // The installer's directory as *this* machine would have it: naming a
        // path the reader does not have is how a refusal stops being actionable.
        let expected = upgrade::installer_dir(host)
            .map(|dir| dir.display().to_string())
            .unwrap_or_else(|_| "its own directory".to_string());

        PerchError::Invalid(format!(
            "Perch is installed at {}, and nothing about that path says which \
             Channel put it there.\n\
             Homebrew and npm keep their binaries somewhere Perch recognizes, \
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
/// Exits nought either way: every other non-zero code Perch has is a refusal,
/// and "there is news" is not one. The Channel is optional here alone, and
/// `null` is the honest answer where the path does not say.
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
                "upgrade_available": behind,
            }),
        );
    }

    say(out, &format!("installed  {installed}"))?;
    say(out, &format!("newest     {newest}"))?;
    say(
        out,
        &format!(
            "channel    {}",
            channel.map_or(
                "unknown (nothing about this binary's path says)",
                |channel| { channel.name() }
            )
        ),
    )?;
    say(
        out,
        match (behind, channel.is_some()) {
            // Said only where it is true: on a binary nothing placed, `perch
            // upgrade` refuses, and pointing at it sends somebody to a refusal.
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
/// Named as a downgrade and told what it costs rather than asked "are you sure":
/// a Perch refuses a registry a newer one wrote, so going back far enough leaves
/// a binary that cannot read its own state.
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
    match said_yes(host, out, "Install the older Release? [y/N] ", Presumed::No)? {
        true => Ok(()),
        false => Err(PerchError::NothingToDo(
            "Nothing was installed.".to_string(),
        )),
    }
}

/// npm would be replacing `perch.exe` while it is the running process, and
/// Windows holds that file open — so the command is printed, to be run from a
/// shell where Perch is not. Spelled from the literals rather than from a
/// resolved `npm`: whether one is on PATH does not bear on this.
fn refuse_npm_replacing_a_running_perch(host: &dyn Host, release: &Option<String>) -> Result<()> {
    if host.platform() != Platform::Windows {
        return Ok(());
    }
    let named = release.as_deref().map(upgrade::version_typed).transpose()?;
    // Nothing done rather than done: `NothingToDo` is already the code for a
    // request understood and a machine left as it was.
    Err(PerchError::NothingToDo(format!(
        "This Installation came from npm, and npm cannot replace `perch.exe` \
         while it is running. Nothing was upgraded.\n\
         Run this from a terminal where Perch is not running:\n\
         \n    npm {}\n",
        upgrade::npm_arguments(named.as_deref()).join(" ")
    )))
}

/// Homebrew installs what the formula says, so `--release` there is a request
/// that cannot be honored.
///
/// Refused rather than quietly ignored: installing the newest instead is the
/// failure somebody finds out about by reading `perch --version` afterwards.
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
/// Said first, because handing the terminal to `brew` for two minutes without
/// saying so reads as a hang. The terminal goes with it — what `brew` and `npm`
/// print is progress — and their exit status is Perch's.
fn hand_it_over(
    host: &dyn Host,
    program: &std::path::Path,
    args: &[String],
    out: &mut dyn Write,
) -> Result<i32> {
    say(out, &crate::host::as_typed(program, args))?;
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    host.exec_interactive(&program.to_string_lossy(), &borrowed, &[])
        .map_err(|err| {
            PerchError::Other(format!(
                "could not run `{}`: {err}",
                crate::host::as_typed(program, args)
            ))
        })
}

/// The one Channel Perch replaces itself for, done by the installer that made
/// the Installation.
///
/// No execute bit, because the script is never run by name: `sh` and
/// `powershell` are handed it as a file, which also works on a `noexec` mount.
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

    // Cleared whichever way it went: what is left behind otherwise is a script
    // a later Perch cannot tell from one it is about to use.
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
            powershell(host)?,
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

/// Where Windows keeps PowerShell: `%SystemRoot%` rather than a literal path or
/// a `PATH` walk, for the reason `curl_bin` gives. A bare name is searched for
/// in the working directory before `PATH` (ADR a-crate-must-not-cost-a-seam),
/// and this one is handed `-ExecutionPolicy Bypass` and a script whose whole job
/// is to overwrite the Perch binary.
fn powershell(host: &dyn Host) -> Result<String> {
    let root = host
        .env_var("SystemRoot")
        .filter(|root| !root.is_empty())
        .ok_or_else(|| {
            PerchError::Other(
                "SystemRoot is unset, so PowerShell cannot be located and the \
                 installer was not run. Nothing was changed."
                    .to_string(),
            )
        })?;
    Ok(format!(
        "{root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
    ))
}
