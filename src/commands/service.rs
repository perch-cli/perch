//! `perch service` — having the machine run the Watcher for you (ADR 0040).
//!
//! Three verbs over one noun, which is the shape `perch group` and `perch
//! config` already have: `install` writes the unit and starts it, `uninstall`
//! stops it and takes the unit back, and `status` says what is there. What a
//! unit *says* is [`crate::service`]'s; putting it on the machine is here.
//!
//! Perch does not background itself and has no `--detach`. What `install`
//! leaves behind is a unit file the platform's own service manager owns, and
//! `uninstall` removes it — reversibility being the line this codebase keeps
//! drawing (ADR 0033, ADR 0034), and the only thing that makes writing to
//! somebody's `~/Library/LaunchAgents` a reasonable thing to do.

use std::io::Write;
use std::path::PathBuf;

use crate::commands::{say, say_json};
use crate::error::{EXIT_NOTHING_TO_DO, EXIT_OK, PerchError, Result};
use crate::host::{Host, Platform};
use crate::service::{self, Driven, Unit};
use crate::{registry, upgrade};

/// What was asked of `perch service`. The help each of these is described by
/// lives with the command line that parses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::Subcommand)]
pub enum ServiceCommand {
    /// Write the unit, start it, and have it start at every login from now on.
    ///
    /// Idempotent: running it again rewrites the unit against the binary that
    /// is there now, which is the repair for a Service that stopped coming up
    /// after an Upgrade moved it.
    Install,

    /// Stop the Service and take its unit back. `perch watch` in a terminal is
    /// unaffected, and nothing Perch holds is touched.
    Uninstall,

    /// Say whether a Service is installed, whether it is running, and whether a
    /// Watcher holds the lock right now.
    ///
    /// A question, so it succeeds either way — branch on `--json` rather than
    /// on the exit code.
    Status {
        /// Report as JSON for whatever is parsing this.
        #[arg(long)]
        json: bool,
    },
}

pub fn run(host: &dyn Host, command: ServiceCommand, out: &mut dyn Write) -> Result<i32> {
    match command {
        ServiceCommand::Install => install(host, out),
        ServiceCommand::Uninstall => uninstall(host, out),
        ServiceCommand::Status { json } => status(host, json, out),
    }
}

/// Writes the unit, starts it, and says what will happen from now on.
///
/// Idempotent, and that is a feature rather than a leniency: re-running it is
/// the documented repair for a unit whose binary has moved, which `perch
/// upgrade` does every time it routes through Homebrew or npm (ADR 0039). So a
/// second install replaces rather than refusing, and says which it did.
fn install(host: &dyn Host, out: &mut dyn Write) -> Result<i32> {
    refuse_as_root(host)?;

    let unit = describe(host)?;
    let at = service::unit_path(host)?;
    let replaced = at.as_deref().is_some_and(|at| host.path_exists(at));

    // The file first, then the service manager: `bootstrap` and `enable` are
    // both given a path that has to be there when they read it.
    if let (Some(at), Some(rendered)) = (&at, unit.rendered(host.platform())) {
        if let Some(parent) = at.parent() {
            host.create_dir_all(parent).map_err(|err| {
                PerchError::file_write(parent, format!("could not make room for the unit: {err}"))
            })?;
        }
        crate::host::write_atomically(host, at, &rendered)
            .map_err(|err| PerchError::file_write(at, err))?;
    }

    // A Windows task is registered rather than written, so there is nothing to
    // undo there — but on the two platforms with a file, a `bootstrap` that
    // fails would otherwise leave a unit behind that starts at the next login
    // having never been checked.
    if let Err(failed) = drive(
        host,
        service::starting(host.platform(), &unit, at.as_deref()),
    ) {
        if let Some(at) = &at {
            let _ = host.remove_file(at);
        }
        return Err(failed.with_note(
            "Nothing was installed, and the unit file was taken back. Perch is \
             unchanged, and `perch watch` in a terminal still works.",
        ));
    }

    say(
        out,
        &format!(
            "{} {} as {}. It starts when you log in, and it is running now.",
            match replaced {
                true => "Replaced the Service, and it now runs",
                false => "Installed the Service. It runs",
            },
            unit.binary.display(),
            service::described(host.platform()),
        ),
    )?;
    say(
        out,
        &format!(
            "Its decisions go to {}.",
            service::log_is_at(host.platform(), unit.log.as_deref()),
        ),
    )?;

    // Said rather than refused, and the reason is the same one ADR 0013 gives
    // for a Margin at or above a Threshold being in range: refusing would make
    // the order two `perch config set`s are typed in matter. A Service with no
    // grant holds harmlessly and takes over the moment one is given (ADR 0040),
    // so the only thing missing is somebody knowing that.
    if let Some(missing) = nothing_may_act(host)? {
        say(out, &missing)?;
    }
    Ok(EXIT_OK)
}

/// Stops the Service and takes the unit back.
fn uninstall(host: &dyn Host, out: &mut dyn Write) -> Result<i32> {
    let at = service::unit_path(host)?;
    let installed = is_installed(host, at.as_deref())?;

    // Every step is allowed to fail, because every one is "make sure this is
    // not running" — so this drives them all and then judges by what is left.
    let _ = drive(host, service::stopping(host.platform(), host.user_id()));

    if let Some(at) = &at
        && host.path_exists(at)
    {
        host.remove_file(at)
            .map_err(|err| PerchError::file_write(at, err))?;
    }

    match installed {
        true => {
            say(
                out,
                "The Service is stopped and its unit is gone. Nothing starts at \
                 login any more, and `perch watch` in a terminal is unaffected.",
            )?;
            Ok(EXIT_OK)
        }
        // The existing code for a request that was already true, which is what
        // this is: a machine with no Service is the machine an `uninstall` was
        // asked to produce.
        false => {
            say(
                out,
                "There is no Service installed, so there was nothing to take \
                 back.",
            )?;
            Ok(EXIT_NOTHING_TO_DO)
        }
    }
}

/// Says what is installed, whether it is running, and whether it can still find
/// the binary it was installed against.
///
/// Exit `0` either way. This is a question, and answering it is success — the
/// same bargain `perch upgrade --check` already makes, which is why a script
/// branches on `--json` rather than on the code.
///
/// It does not read the log. On Linux that would mean shelling out to
/// `journalctl` (ADR 0021 disfavours it) to do worse than `journalctl -f`
/// already does, and it would fork the implementation three ways for a feature
/// every platform already ships.
fn status(host: &dyn Host, json: bool, out: &mut dyn Write) -> Result<i32> {
    let platform = host.platform();
    let at = service::unit_path(host)?;
    let installed = is_installed(host, at.as_deref())?;
    let running = installed && is_running(host);

    // Read off the unit that is actually installed rather than off what one
    // would be written from now, because the whole point of the question is
    // whether those two have come apart (ADR 0039 moves the binary).
    let recorded = installed
        .then(|| recorded_binary(host, at.as_deref()))
        .flatten();
    let binary_is_there = recorded.as_deref().map(|at| host.path_exists(at));
    let watching = watcher_is_running(host);

    if json {
        return say_json(
            out,
            &serde_json::json!({
                "installed": installed,
                "running": running,
                "watching": watching,
                "platform": service::described(platform),
                "unit": at.as_ref().map(|at| at.to_string_lossy()),
                "binary": recorded.as_ref().map(|at| at.to_string_lossy()),
                "binaryExists": binary_is_there,
                "log": service::log_is_at(platform, service::log_path(host)?.as_deref()),
            }),
        )
        .map(|()| EXIT_OK);
    }

    if !installed {
        say(
            out,
            &format!(
                "No Service is installed. `perch service install` has this \
                 machine run the Watcher for you as {}, starting when you log \
                 in.",
                service::described(platform),
            ),
        )?;
    } else {
        say(
            out,
            &format!(
                "A Service is installed as {}, and is {}.",
                service::described(platform),
                match running {
                    true => "running",
                    false => "not running",
                },
            ),
        )?;
        if let Some(at) = &at {
            say(out, &format!("Its unit is {}.", at.display()))?;
        }
        match (&recorded, binary_is_there) {
            (Some(binary), Some(false)) => say(
                out,
                &format!(
                    "It names {}, which is not there any more — an Upgrade moves \
                     the binary (ADR 0039). `perch service install` writes the \
                     unit again against the one that is.",
                    binary.display(),
                ),
            )?,
            (Some(binary), _) => say(out, &format!("It runs {}.", binary.display()))?,
            (None, _) => {}
        }
        say(
            out,
            &format!(
                "Its decisions go to {}.",
                service::log_is_at(platform, service::log_path(host)?.as_deref()),
            ),
        )?;
    }

    // The Service and the Watcher are different questions, and a machine can
    // answer them differently: a Service that is installed and stopped, a
    // `perch watch` somebody typed in a terminal, or the moment after a Service
    // has been told to start and before it has taken the lock.
    say(
        out,
        match watching {
            true => "A Watcher is running on this machine and holds the watcher lock.",
            false => "No Watcher is running on this machine.",
        },
    )?;

    if let Some(missing) = nothing_may_act(host)? {
        say(out, &missing)?;
    }
    Ok(EXIT_OK)
}

/// Stops the Service and takes its unit back, before a Purge deletes anything
/// (ADR 0040).
///
/// This is ADR 0024's shape one level up. Removing the active Account lands
/// somewhere before it deletes anything; giving the whole machine back stops the
/// thing that writes to it before it starts. A Watcher racing a Purge is one
/// process writing a captured Credential into a Profile directory another is
/// deleting, and there is no partial success there worth having.
///
/// **Refuses rather than continuing** where the Service will not stop. A Purge
/// that carried on would leave a supervised loop coming straight back at a
/// machine whose registry is being deleted underneath it — and on every platform
/// the supervisor would restart it, so "it will be gone in a moment" is not
/// true.
///
/// Answers whether there was one, so the report can say so. Silent where there
/// is not: most machines have no Service, and a Purge narrating what it did not
/// find would bury what it did.
pub fn take_back_before_a_purge(host: &dyn Host, out: &mut dyn Write) -> Result<bool> {
    let at = service::unit_path(host)?;
    if !is_installed(host, at.as_deref())? {
        return Ok(false);
    }

    let _ = drive(host, service::stopping(host.platform(), host.user_id()));

    // Asked rather than assumed, because every step of `stopping` is allowed to
    // fail and this is the one caller for whom that is not good enough: an
    // `uninstall` judges by what is left, and a Purge has to judge by what is
    // still running.
    if is_running(host) {
        return Err(PerchError::Busy(format!(
            "The Service is still running, so nothing was purged.\n\
             It would go on Switching Credentials into Profiles this command is \
             deleting. Stop it with `perch service uninstall` and run this \
             again — it is {} and something is refusing to stop it.",
            service::described(host.platform()),
        )));
    }

    if let Some(at) = &at
        && host.path_exists(at)
    {
        host.remove_file(at)
            .map_err(|err| PerchError::file_write(at, err))?;
    }

    say(
        out,
        "The Service is stopped and its unit is gone, before anything else was \
         touched.",
    )?;
    Ok(true)
}

/// Whether a Service is installed at all, for the commands that have to mention
/// one without managing it.
pub fn is_there(host: &dyn Host) -> bool {
    service::unit_path(host)
        .and_then(|at| is_installed(host, at.as_deref()))
        .unwrap_or(false)
}

/// Writes the unit again against the binary that is there now, after an Upgrade
/// has moved it (ADR 0039, ADR 0040).
///
/// Answers what to say rather than saying it, and never fails: the Upgrade
/// itself succeeded, the binary really is newer, and a Service that could not be
/// refreshed is a warning with a one-command repair rather than a reason to
/// report that the Upgrade did not happen.
pub fn refreshed_after_an_upgrade(host: &dyn Host) -> Option<String> {
    if !is_there(host) {
        return None;
    }

    let refreshed = describe(host).and_then(|unit| {
        let at = service::unit_path(host)?;
        if let (Some(at), Some(rendered)) = (&at, unit.rendered(host.platform())) {
            crate::host::write_atomically(host, at, &rendered)
                .map_err(|err| PerchError::file_write(at, err))?;
        }
        drive(
            host,
            service::starting(host.platform(), &unit, at.as_deref()),
        )?;
        Ok(unit.binary)
    });

    Some(match refreshed {
        Ok(binary) => format!(
            "The Service was restarted, and now runs {}.",
            binary.display(),
        ),
        // Named as a warning with its repair, because the machine is in a state
        // somebody can act on: the old binary may be gone, so the Service may
        // not come up at the next login.
        Err(why) => format!(
            "The Service could not be restarted against the new binary: {why}\n\
             Perch itself upgraded. Run `perch service install` to point the \
             Service at it — until then it may not come up when you log in.",
        ),
    })
}

/// Everything a unit would be written from, as this machine stands now.
fn describe(host: &dyn Host) -> Result<Unit> {
    let exe = host
        .current_exe()
        .map_err(|err| PerchError::Other(format!("could not find Perch's own binary: {err}")))?;

    Ok(Unit {
        binary: service::binary_for_the_unit(&exe, upgrade::channel(host)?.as_ref()),
        environment: service::CARRIED
            .iter()
            .filter_map(|key| host.env_var(key).map(|value| (key.to_string(), value)))
            .collect(),
        log: service::log_path(host)?,
        user_id: host.user_id(),
    })
}

/// Whether a Service is installed, asked of whatever this platform keeps one in.
///
/// A file on the two platforms that have one, and the service manager itself on
/// Windows, where the task lives in Windows' own store and there is nothing on
/// disk to look at.
fn is_installed(host: &dyn Host, at: Option<&std::path::Path>) -> Result<bool> {
    match at {
        Some(at) => Ok(host.path_exists(at)),
        None => Ok(host.platform() == Platform::Windows && is_running(host)),
    }
}

/// Whether the service manager says it is running right now.
fn is_running(host: &dyn Host) -> bool {
    let Some(asking) = service::asking(host.platform(), host.user_id()) else {
        return false;
    };
    let args: Vec<&str> = asking.args.iter().map(String::as_str).collect();
    host.exec(&asking.program, &args)
        .map(|ran| ran.succeeded())
        .unwrap_or(false)
}

/// Whether *a Watcher* is running, which is a different question from whether
/// the Service is.
///
/// Asked of the watcher lock rather than of the process table (ADR 0040). A lock
/// is the thing a Watcher actually holds, it is held for exactly as long as one
/// runs, and it is given back however the process ends — where scanning
/// processes for something that looks like Perch is defeated by a renamed
/// binary, races the thing it is asking about, and has no good answer on
/// Windows.
///
/// Read by trying to take it and giving it straight back, which is the one way
/// to ask that cannot be raced: `mkdir` either creates the directory or fails,
/// with nothing in between.
fn watcher_is_running(host: &dyn Host) -> bool {
    let Ok(spec) = registry::watcher_lock_spec(host) else {
        return false;
    };
    match crate::lock::take_all(host, vec![spec]) {
        // Taken, so nobody had it — and it is given back on the way out of this
        // function, when the hold is dropped.
        Ok(_taken) => false,
        Err(_) => true,
    }
}

/// The binary the *installed* unit names, read back out of it.
///
/// Read rather than recomputed, because the question `status` is answering is
/// whether the unit and the machine have come apart — and a value worked out
/// again from the machine would agree with the machine by construction.
///
/// `None` where there is nothing to read: Windows keeps no file, and a unit
/// somebody has edited into a shape Perch does not recognise is one Perch
/// declines to make claims about rather than guessing at.
fn recorded_binary(host: &dyn Host, at: Option<&std::path::Path>) -> Option<PathBuf> {
    let text = host.read_file(at?).ok()?;
    match host.platform() {
        Platform::Other => text
            .lines()
            .find_map(|line| line.strip_prefix("ExecStart="))
            .and_then(|line| line.strip_suffix(" watch"))
            .map(PathBuf::from),
        // The first `<string>` inside `ProgramArguments`, which is the program.
        Platform::MacOs => {
            let array = text.split("<key>ProgramArguments</key>").nth(1)?;
            let opened = array.find("<string>")? + "<string>".len();
            let closed = array[opened..].find("</string>")? + opened;
            Some(PathBuf::from(unescaped(&array[opened..closed])))
        }
        Platform::Windows => None,
    }
}

/// The inverse of the plist escaping, for reading a path back out of one.
fn unescaped(value: &str) -> String {
    value
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        // Last, so that an `&amp;lt;` in somebody's path survives the round trip
        // rather than becoming a `<` on the way back.
        .replace("&amp;", "&")
}

/// The line saying a Service will hold because no Scope has granted anything,
/// or `None` where one has.
///
/// Asked of the whole registry rather than of the active Account, because a
/// Service outlives whichever Account happens to be active when it is installed:
/// somebody with one Group that has said yes has arranged their machine, even if
/// they are sitting on an ungrouped Account this minute.
fn nothing_may_act(host: &dyn Host) -> Result<Option<String>> {
    // Nothing to say about a registry that will not load, or one that is not
    // there yet — whatever is wrong with it is a bigger piece of news than this
    // line, and the command that needs it will say so.
    let Ok(Some(registry)) = registry::load(host) else {
        return Ok(None);
    };

    let any = registry.global.settings.watcher_may_act
        || registry.groups.keys().any(|group| {
            registry
                .in_force(&registry::Scope::Group(group.clone()))
                .watcher_may_act
        })
        || registry
            .in_force(&registry::Scope::Ungrouped)
            .watcher_may_act;
    if any {
        return Ok(None);
    }

    Ok(Some(
        "No Scope has told the Watcher it may act, so the Service will hold \
         rather than decide anything. `perch config set <group> watcher-may-act \
         true` is what starts it deciding, and it takes effect within a couple \
         of minutes without anything being restarted."
            .to_string(),
    ))
}

/// Runs what the platform's service manager has to be told, in order.
fn drive(host: &dyn Host, steps: Vec<Driven>) -> Result<()> {
    for step in steps {
        let args: Vec<&str> = step.args.iter().map(String::as_str).collect();
        let ran = host.exec(&step.program, &args);
        if !step.required {
            continue;
        }
        let failed = match ran {
            Ok(ran) if ran.succeeded() => continue,
            // What the service manager said, rather than a code: `systemctl`
            // and `launchctl` both explain themselves, and a sentence naming
            // the unit is worth more than "exited 1".
            Ok(ran) => match ran.stderr.trim().is_empty() {
                true => ran.stdout.trim().to_string(),
                false => ran.stderr.trim().to_string(),
            },
            Err(err) => format!("{err} — is `{}` on your PATH?", step.program),
        };
        return Err(PerchError::Other(format!(
            "`{}` failed: {failed}",
            step.as_typed(),
        )));
    }
    Ok(())
}

/// Refuses to install a Service for somebody who is not the person it would
/// watch (ADR 0040).
///
/// Every Profile is under one person's home directory and the registry that says
/// which Account is active is theirs. Installed under `sudo`, the Service would
/// be a root process watching root's registry — which is empty — while the
/// person who typed it went on wondering why nothing ever switched. On macOS it
/// would additionally have no unlocked keychain to read (ADR 0008).
fn refuse_as_root(host: &dyn Host) -> Result<()> {
    if host.user_id() != Some(0) {
        return Ok(());
    }
    Err(PerchError::Invalid(
        "A Service belongs to one person, and this is running as root.\n\
         Every Profile Perch holds is under a home directory, so a Service \
         installed this way would watch root's registry rather than yours. Run \
         `perch service install` as yourself, without `sudo`."
            .to_string(),
    ))
}
