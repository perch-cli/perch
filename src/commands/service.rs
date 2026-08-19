//! `perch watcher install`, `uninstall` and `status` — having the machine run
//! the Watcher for you (ADR 0040).
//!
//! Three of the Watcher's five verbs, and the three that are about the Service:
//! `install` writes the unit and starts it, `uninstall` stops it and takes the
//! unit back, and `status` says what is there. Which command line reaches which
//! is [`crate::commands::watcher`]'s; what a unit *says* is
//! [`crate::service`]'s; putting it on the machine is here.
//!
//! The Service keeps its name here because it keeps its glossary entry: it
//! names an arrangement of the Watcher, and what lost its tree is the CLI noun
//! rather than the concept (ADR 0047).
//!
//! Perch does not background itself and has no `--detach`. What `install`
//! leaves behind is a unit file the platform's own service manager owns, and
//! `uninstall` removes it — reversibility being the line this codebase keeps
//! drawing (ADR 0033, ADR 0034), and the only thing that makes writing to
//! somebody's `~/Library/LaunchAgents` a reasonable thing to do.

use std::io::Write;
use std::path::PathBuf;

use crate::commands::{say, say_json};
use crate::cycle;
use crate::error::{EXIT_NOTHING_TO_DO, EXIT_OK, PerchError, Result};
use crate::host::{Host, Platform};
use crate::service::{self, Driven, Unit};
use crate::{registry, upgrade};

/// Writes the unit, starts it, and says what it did.
///
/// Idempotent, and that is a feature rather than a leniency: re-running it is
/// the documented repair for a unit whose binary has moved, which `perch
/// upgrade` does every time it routes through Homebrew or npm (ADR 0039). So a
/// second install replaces rather than refusing, and says which it did.
pub fn install(host: &dyn Host, out: &mut dyn Write) -> Result<i32> {
    refuse_as_root(host)?;

    let unit = describe(host)?;
    // Before the machine is asked anything and before anything is written. A
    // value no format can hold is a refusal about the Unit, not a half-finished
    // install to take back — and the line below asks the service manager a
    // question, which on Windows is a `schtasks` this refusal must come ahead
    // of. `write_and_start` asks it again as its own precondition, because the
    // other door into it has no such ordering to keep; reaching it from here
    // means this one already answered.
    unit.refuse_what_the_format_cannot_hold(host.platform())?;
    let at = service::unit_path(host)?;
    // Asked the way `status` asks it, rather than of the file alone. Windows
    // keeps no unit file — `unit_path` is `None` there and the task lives in
    // Windows' own store — so a re-install over a working Scheduled Task always
    // reported "Installed the Service", and a `schtasks /Create` that then
    // failed took the rollback arm written for an install that *made* something,
    // which is the arm this whole comment block below says must not run over one
    // that was already there.
    let replaced = is_installed(host, at.as_deref())?;

    // A Windows task is registered rather than written, so there is nothing to
    // undo there — but on the two platforms with a file, a `bootstrap` that
    // fails would otherwise leave a unit behind that starts at the next login
    // having never been checked.
    //
    // Only where this install *made* the unit. A re-install is the documented
    // repair after an Upgrade, so the common way to reach this is with a working
    // Service already installed — and taking the file back then left the machine
    // worse than it found it: the unit was already overwritten by the time the
    // start was attempted, so removing it takes away the Service that was there,
    // under a sentence promising Perch is unchanged. A start that fails for
    // something outside Perch, like a `systemctl --user` with no session bus
    // over SSH, would silently uninstall a Watcher that had been running for
    // months. The replaced unit is left where it is and said so instead.
    if let Err(failed) = write_and_start(host, &unit, at.as_deref()) {
        // Named for what this platform keeps, because Windows keeps no file and
        // a sentence about one is a sentence pointing at nothing. What is true
        // on all three is that something is registered and was not started.
        let kept = match at.is_some() {
            true => "The unit file has been replaced and is left where it is",
            false => "What was registered is left where it is",
        };
        if replaced {
            return Err(failed.with_note(&format!(
                "The Service was not started. {kept}, so it starts at the next \
                 login — `perch watcher status` says what is there now, and \
                 `perch watcher uninstall` takes it away.",
            )));
        }
        if let Some(at) = &at {
            let _ = host.remove_file(at);
        }
        return Err(failed.with_note(&format!(
            "Nothing was installed{}. Perch is unchanged, and `perch watcher \
             run` in a terminal still works.",
            match at.is_some() {
                true => ", and the unit file was taken back",
                false => "",
            },
        )));
    }

    // What it did and which binary it baked in, and nothing about starting at
    // login: an install that succeeded always did that, and the failure arms
    // above are where a Service that is not running says so (ADR 0061).
    //
    // The log stays, and the two are not the same kind of line. Where a
    // Service writes is derived from `PERCH_HOME` and differs by platform, so
    // it is a datum the person could not have predicted rather than a step
    // being narrated — and it is the one thing they need in order to read what
    // the Service goes on to decide.
    say(
        out,
        &format!(
            "{} {} as {}.",
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
pub fn uninstall(host: &dyn Host, out: &mut dyn Write) -> Result<i32> {
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
            // Nothing about what stops starting at login and nothing about
            // the terminal being unaffected: an uninstall that succeeded
            // always did the first, and the second is a worry Perch was
            // pre-empting rather than a thing that happened (ADR 0061).
            say(out, "The Service is stopped and its unit is gone.")?;
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
/// `journalctl` (ADR 0021 disfavors it) to do worse than `journalctl -f`
/// already does, and it would fork the implementation three ways for a feature
/// every platform already ships.
pub fn status(host: &dyn Host, json: bool, out: &mut dyn Write) -> Result<i32> {
    let platform = host.platform();
    let at = service::unit_path(host)?;
    let installed = is_installed(host, at.as_deref())?;
    let watching = watcher_is_running(host);
    // Windows is asked differently, because `schtasks /Query` answers whether
    // the task *exists* rather than whether it is running — which is the same
    // question `is_installed` asks it, so `installed && is_running` was true by
    // construction and a logon task that had not fired since boot read as
    // running. The evidence Windows does have is the watcher lock: a Watcher
    // holds it for exactly as long as one runs, and the task's whole action is
    // to be one. It cannot tell that Watcher from a `perch watcher run`
    // somebody typed, and that is the smaller error of the two.
    let running = match platform {
        Platform::Windows => installed && watching,
        _ => installed && is_running(host),
    };

    // Read off the unit that is actually installed rather than off what one
    // would be written from now, because the whole point of the question is
    // whether those two have come apart (ADR 0039 moves the binary).
    let recorded = installed
        .then(|| recorded_binary(host, at.as_deref()))
        .flatten();
    let binary_is_there = recorded.as_deref().map(|at| host.path_exists(at));
    let log = recorded_log(host, at.as_deref(), installed)?;

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
                "log": service::log_is_at(platform, log.as_deref()),
            }),
        )
        .map(|()| EXIT_OK);
    }

    if !installed {
        say(
            out,
            &format!(
                "No Service is installed. `perch watcher install` has this \
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
                    "It names {}, which is not there any more — an Upgrade \
                     moves the binary (ADR 0039). `perch watcher install` \
                     writes the unit again against the one that is.",
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
                service::log_is_at(platform, log.as_deref()),
            ),
        )?;
    }

    // The Service and the Watcher are different questions, and a machine can
    // answer them differently: a Service that is installed and stopped, a
    // `perch watcher run` somebody typed in a terminal, or the moment after a
    // Service has been told to start and before it has taken the lock.
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

    // Asked before the Service question rather than after it, which is where it
    // used to sit and could not be reached. A Watcher started with `perch
    // watcher run` in a terminal holds the same lock, is the same hazard — it
    // Switches Credentials into the Profiles this command is deleting — and is
    // on a machine where no Service was ever installed, so the early return
    // below skipped the one check that would have seen it.
    //
    // The lock is what answers, for the reason the Service branch below gives
    // at length: it is held for exactly as long as a Watcher runs and given
    // back however the process ends.
    if !is_installed(host, at.as_deref())? {
        if watcher_is_running(host) {
            return Err(PerchError::Busy(
                "A Watcher is running, so nothing was purged.\n                 It would go on Switching Credentials into Profiles this command                  is deleting. Stop it — Ctrl-C in the terminal running `perch                  watcher run` — and run this again."
                    .to_string(),
            ));
        }
        return Ok(false);
    }

    let _ = drive(host, service::stopping(host.platform(), host.user_id()));

    // Asked rather than assumed, because every step of `stopping` is allowed to
    // fail and this is the one caller for whom that is not good enough: an
    // `uninstall` judges by what is left, and a Purge has to judge by what is
    // still running.
    //
    // The watcher lock first, and on every platform. What this guard is about is
    // a *Watcher* writing a captured Credential into a Profile the Purge is
    // deleting, and the lock is the only thing that answers that question
    // directly: it is held for exactly as long as a Watcher runs and given back
    // however the process ends. The service manager answers a question one step
    // away from it — on Linux a unit can read `inactive` while the process it
    // started is still winding down mid-Switch, and on Windows `schtasks /Query`
    // answers whether the task *exists*, which `/Delete` has just made false
    // whether or not it killed anything. `status` has known that about Windows
    // for as long as it has been asked; this copy of the question did not, so
    // the one platform where `stopping` cannot terminate a running instance was
    // also the one where the guard could not see it.
    if watcher_is_running(host) || (host.platform() != Platform::Windows && is_running(host)) {
        return Err(PerchError::Busy(format!(
            "The Service is still running, so nothing was purged.\n\
             It would go on Switching Credentials into Profiles this command is \
             deleting. Stop it with `perch watcher uninstall` and run this \
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

    say(out, "The Service is stopped and its unit is gone.")?;
    Ok(true)
}

/// Whether a Service is installed at all, for the commands that have to mention
/// one without managing it.
pub fn is_there(host: &dyn Host) -> bool {
    service::unit_path(host)
        .and_then(|at| is_installed(host, at.as_deref()))
        .unwrap_or(false)
}

/// Puts the unit on disk and hands it to the service manager: the whole of what
/// "install this Service" means on every platform that keeps a file, and nothing
/// at all on the one that does not.
///
/// One function because there are two doors to it — an `install` somebody typed
/// and the refresh an Upgrade owes a Service whose binary has moved — and the
/// second of them had assembled the sequence by hand, minus a clause. The guard
/// was the clause: `install` refuses a Unit carrying a value the format cannot
/// hold *before* it writes anything, and the Upgrade path wrote
/// `unit.rendered(...)` straight out. Both read `PERCH_HOME` and
/// `CLAUDE_CONFIG_DIR` off the environment they are run in, so a `PERCH_HOME`
/// with a newline in it closed `Environment=` and appended arbitrary directives
/// to a unit systemd loads at every login — the exact hazard
/// [`Unit::refuse_what_the_format_cannot_hold`] exists for, reached through the
/// door that did not ask it.
///
/// The file first, then the service manager: `bootstrap` and `enable` are both
/// given a path that has to be there when they read it.
fn write_and_start(host: &dyn Host, unit: &Unit, at: Option<&std::path::Path>) -> Result<()> {
    // Before the parent directory is made and before anything is written: a
    // value no format can hold is a refusal about the Unit, not a half-finished
    // install to take back.
    unit.refuse_what_the_format_cannot_hold(host.platform())?;

    // The directory the decision log goes in, before anything is told to write
    // there. It is inside Perch's home, which is made on the way to the first
    // lock Perch takes — and neither door here reads the registry through
    // anything that takes one, so on a machine where Claude Code is logged in
    // and Perch has never run, the directory was simply not there. `cmd /c … >>
    // "…\watch.log"` cannot open a redirect into a directory that does not
    // exist, so the Windows task failed at every logon, silently; launchd cannot
    // open `StandardOutPath` either.
    //
    // Private, because it is Perch's own home rather than a path somebody
    // typed, and a Purge sweeps it with everything else Perch holds.
    if let Some(log) = unit.log.as_deref().and_then(std::path::Path::parent) {
        host.create_private_dir_all(log).map_err(|err| {
            PerchError::file_write(log, format!("could not make room for the log: {err}"))
        })?;
    }

    if let (Some(at), Some(rendered)) = (at, unit.rendered(host.platform())) {
        if let Some(parent) = at.parent() {
            host.create_dir_all(parent).map_err(|err| {
                PerchError::file_write(parent, format!("could not make room for the unit: {err}"))
            })?;
        }
        crate::host::write_atomically(host, at, &rendered)
            .map_err(|err| PerchError::file_write(at, err))?;
    }

    drive(host, service::starting(host.platform(), unit, at))
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
        write_and_start(host, &unit, at.as_deref())?;
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
             Perch itself upgraded. Run `perch watcher install` to point the \
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
        // Off the machine, because `schtasks` has no notation for "whoever is
        // running this" and the one Perch was passing — `%USERNAME%` — is
        // `cmd.exe`'s, expanded by a shell that is not there.
        user_name: host.env_var("USERNAME"),
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

/// The service-manager binary, spelled the way ADR 0021 asks on the platform
/// that needs it.
///
/// `commands::upgrade::powershell` states the rule and the reason: "A bare name
/// handed to Windows is searched for in the application directory and *the
/// current working directory* before `PATH`." `schtasks` is the higher-value
/// target of the two it applies to — its `/TR` argument is a command line
/// Windows runs at every logon, so a `schtasks.exe` dropped in a downloads
/// folder turns `perch watcher install` typed there into attacker-chosen
/// persistence.
///
/// Windows alone. `launchctl` and `systemctl` are found through `PATH`, which
/// on unix does not include the working directory unless somebody has put it
/// there, and there is no fixed absolute path for `systemctl` that holds across
/// distributions — `/bin` on some, `/usr/bin` on others. Spelling one would
/// trade a hazard this platform does not have for a machine Perch stops
/// working on.
fn located(host: &dyn Host, program: &str) -> String {
    if host.platform() != Platform::Windows {
        return program.to_string();
    }
    match host.env_var("SystemRoot").filter(|root| !root.is_empty()) {
        Some(root) => format!("{root}\\System32\\{program}.exe"),
        // Said by the failure rather than refused here: every caller of this
        // already reports what would not run, and a Windows with no
        // `SystemRoot` has worse problems than this one.
        None => program.to_string(),
    }
}

/// Whether the service manager says it is running right now.
fn is_running(host: &dyn Host) -> bool {
    let Some(asking) = service::asking(host.platform(), host.user_id()) else {
        return false;
    };
    let args: Vec<&str> = asking.args.iter().map(String::as_str).collect();
    host.exec(&located(host, &asking.program), &args)
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
    // Asked rather than requested. `take_all` answers this too, but only by
    // exhausting its five attempts first — so the machine this command exists
    // for, the one with a Service running, was the one that sat for four
    // seconds saying nothing before it printed a word.
    //
    // A lock that cannot be asked about at all is not contention and is not
    // read as it — a parent directory that cannot be created, an artifact whose
    // time will not be read. Read as a holder it told the user a Watcher is
    // running on a machine where none is, which on Windows also makes `running`
    // true. The rest of Perch tells a refusal from a fault everywhere it asks;
    // this was the one place that folded them together.
    crate::lock::is_held(host, &spec).unwrap_or(false)
}

/// The binary the *installed* unit names, read back out of it.
///
/// Read rather than recomputed, because the question `status` is answering is
/// whether the unit and the machine have come apart — and a value worked out
/// again from the machine would agree with the machine by construction.
///
/// The reading is [`service::binary_in`]'s, beside the writing it is the
/// inverse of. What is left here is the one effect: a file off the machine,
/// with `None` where there is nothing to read at all.
fn recorded_binary(host: &dyn Host, at: Option<&std::path::Path>) -> Option<PathBuf> {
    let text = host.read_file(at?).ok()?;
    service::binary_in(host.platform(), &text)
}

/// Where the decision log goes, asked of the unit that is installed.
///
/// The same rule as [`recorded_binary`], applied to the other thing an install
/// bakes into the unit. [`service::log_path`] derives the path from `PERCH_HOME`
/// as it stands *now*, so a Service installed under one and a `status` run under
/// another named a file nothing writes to, confidently, while the real log went
/// on filling up where the install had put it.
///
/// Falls back to that derivation only where there is nothing to read back: a
/// machine with no Service installed, where what one would be written with is
/// the honest answer, and Windows, which registers a task rather than writing a
/// unit — the same platform [`recorded_binary`] can say nothing about either.
fn recorded_log(
    host: &dyn Host,
    at: Option<&std::path::Path>,
    installed: bool,
) -> Result<Option<PathBuf>> {
    if !installed || host.platform() == Platform::Windows {
        return service::log_path(host);
    }
    Ok(at
        .and_then(|at| host.read_file(at).ok())
        .and_then(|text| service::log_in(host.platform(), &text)))
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

    // Both statements, because a grant alone is not enough for the Ungrouped
    // Accounts: `perch watcher run` asks `interchangeable` first and holds
    // without it (ADR 0017). Read from `watcher-may-act` alone, this line stayed
    // silent on a machine whose only grant is one the Watcher will never act on
    // — the same claim `perch group list` was making until it took the gate on
    // (`commands::group`), and `config::scope_lines` already answers correctly.
    let any = registry.scopes().iter().any(|scope| {
        registry.settings(scope).watcher_may_act && cycle::may_cycle_within(&registry, scope)
    });
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
        let ran = host.exec(&located(host, &step.program), &args);
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
         `perch watcher install` as yourself, without `sudo`."
            .to_string(),
    ))
}
