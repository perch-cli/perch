//! `perch watcher install`, `uninstall` and `status` — having the machine run the
//! Watcher for you (ADR the-machine-runs-the-watcher).
//!
//! `install` writes the unit and starts it, `uninstall` stops it and takes the unit
//! back, and `status` says what is there. What a unit *says* is [`crate::service`]'s.
//!
//! What `install` leaves behind is a unit `uninstall` takes back, which is what makes
//! writing to somebody's `~/Library/LaunchAgents` reasonable at all
//! (ADR perch-takes-back-what-it-wrote).

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::cycle;
use crate::error::{EXIT_NOTHING_TO_DO, EXIT_OK, PerchError, Result};
use crate::holdings;
use crate::host::{Host, Platform};
use crate::say;
use crate::service::{self, Driven, Manager, Standing, Unit};
use crate::{registry, upgrade};

/// Writes the unit, starts it, and says what it did.
///
/// Idempotent, because re-running it is the documented repair for a unit whose binary
/// has moved (ADR an-upgrade-asks-its-channel), so a second install replaces rather
/// than refusing — and says which it did.
pub fn install(host: &dyn Host, out: &mut dyn Write) -> Result<i32> {
    refuse_as_root(host)?;
    let manager = Manager::of(host);

    let unit = describe(host)?;
    // Before the machine is asked anything and before anything is written: a value no
    // format can hold is a refusal about the Unit rather than a half-finished install,
    // and `is_installed` below runs `schtasks` on Windows.
    unit.refuse_what_the_format_cannot_hold(manager)?;
    let at = manager.unit_path(host)?;
    // Asked the way `status` asks it rather than of the file alone, because Windows
    // keeps no unit file and a re-install over a working task is not an install that
    // made something.
    let replaced = is_installed(host, at.as_deref())?;

    if let Err(failed) = write_and_start(host, &unit, at.as_deref()) {
        // Named for what this platform keeps, because Windows keeps no file. What is
        // true on all three is that something is registered and was not started.
        let kept = match at.is_some() {
            true => "The unit file has been replaced and is left where it is",
            false => "What was registered is left where it is",
        };
        if replaced {
            return Err(failed.with_note(&format!(
                "The Service was not started. {kept}, so it starts at the next \
                 login. `perch watcher status` says what is there now, and \
                 `perch watcher uninstall` takes it away.",
            )));
        }
        if let Some(at) = &at {
            // In `uninstall`'s order, for the reason `service::forgetting` gives:
            // `enable --now` makes the wants-symlink and *then* starts, so the
            // disable has to reach a unit systemd can still resolve.
            let _ = drive(host, manager.stopping(host.user_id()));
            let _ = host.remove_file(at);
            let _ = drive(host, manager.forgetting());
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

    // What it did and which binary it baked in, and nothing about starting at login
    // (ADR perch-says-what-it-did). The log stays, because where a Service writes
    // differs by platform.
    say::line(
        out,
        &format!(
            "{} {} as {}.",
            match replaced {
                true => "Replaced the Service, and it now runs",
                false => "Installed the Service. It runs",
            },
            unit.binary.display(),
            manager.described(),
        ),
    )?;
    say::line(
        out,
        &format!(
            "Its decisions go to {}.",
            manager.log_is_at(unit.log.as_deref()),
        ),
    )?;

    if any_scope_may_act(host) == Some(false) {
        say::line(out, service::HOLDS_FOR_A_GRANT)?;
    }
    Ok(EXIT_OK)
}

/// Stops the Service and takes the unit back.
pub fn uninstall(host: &dyn Host, out: &mut dyn Write) -> Result<i32> {
    let manager = Manager::of(host);
    let at = manager.unit_path(host)?;
    let installed = is_installed(host, at.as_deref())?;

    // Every step is allowed to fail, because every one is "make sure this is not
    // running", so this drives them all and then judges by what is left.
    let _ = drive(host, manager.stopping(host.user_id()));

    take_the_unit_back(host, at.as_deref())?;

    // Asked again rather than reported off the `installed` above: every step
    // above may fail, and a refused `schtasks /Delete` leaves a task that runs
    // at every logon while this says it is gone.
    if is_installed(host, at.as_deref())? {
        return Err(PerchError::Busy(format!(
            "The Service is still installed, so it was not taken back.\n\
             It is {} and something is refusing to unregister it.",
            manager.described(),
        )));
    }

    match installed {
        true => {
            say::line(out, TAKEN_BACK)?;
            Ok(EXIT_OK)
        }
        // The code for a request that was already true: a machine with no Service is
        // the machine an `uninstall` was asked to produce.
        false => {
            say::line(
                out,
                "There is no Service installed, so there was nothing to take \
                 back.",
            )?;
            Ok(EXIT_NOTHING_TO_DO)
        }
    }
}

/// Says what is installed, whether it is running, and whether its binary is there.
///
/// Exit `0` either way, because answering a question is success. It does not read the
/// log: that would mean shelling out to `journalctl` three ways (ADR
/// a-crate-must-not-cost-a-seam).
pub fn status(host: &dyn Host, json: bool, out: &mut dyn Write) -> Result<i32> {
    let standing = asked_of_the_machine(host)?;
    match json {
        true => say::json(out, &standing.document())?,
        false => {
            for line in standing.lines() {
                say::line(out, &line)?;
            }
        }
    }
    Ok(EXIT_OK)
}

/// Every question this command puts to the machine, put once.
///
/// The answers travel as a [`Standing`] rather than as locals, so the document and
/// the sentences are two renderings of one reading rather than two readings.
pub fn asked_of_the_machine(host: &dyn Host) -> Result<Standing> {
    let manager = Manager::of(host);
    let at = manager.unit_path(host)?;
    let installed = is_installed(host, at.as_deref())?;
    let watching = watcher_is_running(host);
    // Asked of the manager where asking it means anything. Where it does not, the
    // question it answers is whether the task *exists*, which is what `is_installed`
    // already asked it — so the only evidence left is the watcher lock.
    let running = installed
        && match manager.keeps_its_own_answer() {
            true => is_running(host),
            false => watching,
        };

    // Read off the unit that is actually installed rather than off what one would be
    // written from now, because whether those two have come apart is the question.
    // Once, because both answers below come out of the same file.
    let unit = at.as_deref().and_then(|at| host.read_file(at).ok());
    let recorded = installed
        .then(|| unit.as_deref().and_then(|text| manager.binary_in(text)))
        .flatten();

    Ok(Standing {
        manager,
        installed,
        running,
        watching,
        binary_is_there: recorded.as_deref().map(|at| host.path_exists(at)),
        log: recorded_log(host, unit.as_deref(), installed)?,
        binary: recorded,
        unit: at,
        any_scope_may_act: any_scope_may_act(host),
    })
}

/// Stops the Service and takes its unit back, before a Purge deletes anything.
///
/// ADR a-removal-lands-first's shape one level up, and it **refuses rather than
/// continuing** where the Service will not stop: a Watcher racing a Purge writes a
/// captured Credential into a Profile directory another process is deleting.
pub fn take_back_before_a_purge(
    host: &dyn Host,
    out: &mut dyn Write,
    _fresh: &crate::wait::Fresh,
) -> Result<bool> {
    let manager = Manager::of(host);
    let at = manager.unit_path(host)?;

    // Asked before the Service question rather than after it: a Watcher somebody typed
    // holds the same lock and is the same hazard, on a machine where no Service was
    // ever installed and the early return below is taken.
    if !is_installed(host, at.as_deref())? {
        if watcher_is_running(host) {
            return Err(PerchError::Busy(
                "A Watcher is running, so nothing was purged.\n\
                 It would go on Switching Credentials into Profiles this command \
                 is deleting. Ctrl-C in the terminal running `perch watcher \
                 run` stops it, then run this again."
                    .to_string(),
            ));
        }
        return Ok(false);
    }

    let _ = drive(host, manager.stopping(host.user_id()));

    // Asked rather than assumed, because every step of `stopping` may fail and this
    // caller judges by what is still running — the watcher lock first, because a unit
    // reads `inactive` while the process it started winds down mid-Switch.
    if watcher_is_running(host) || still_held_by_the_service_manager(host) {
        return Err(PerchError::Busy(format!(
            "The Service is still running, so nothing was purged.\n\
             It would go on Switching Credentials into Profiles this command is \
             deleting. Stop it with `perch watcher uninstall` and run this \
             again. It is {} and something is refusing to stop it.",
            manager.described(),
        )));
    }

    take_the_unit_back(host, at.as_deref())?;

    say::line(out, TAKEN_BACK)?;
    Ok(true)
}

/// The unit file gone and the service manager told to forget it — one step,
/// because `forgetting` goes after the removal, which is the whole reason it is
/// not part of `stopping`. What is left is the caller's to judge.
fn take_the_unit_back(host: &dyn Host, at: Option<&Path>) -> Result<()> {
    if let Some(at) = at
        && host.path_exists(at)
    {
        host.remove_file(at)
            .map_err(|err| PerchError::file_write(at, err))?;
    }
    let _ = drive(host, Manager::of(host).forgetting());
    Ok(())
}

/// What both ways of taking a Service back say when it is gone.
const TAKEN_BACK: &str = "The Service is stopped and its unit is gone.";

/// Whether a Service is installed at all, for the commands that have to mention one
/// without managing it.
pub fn is_there(host: &dyn Host) -> bool {
    Manager::of(host)
        .unit_path(host)
        .and_then(|at| is_installed(host, at.as_deref()))
        .unwrap_or(false)
}

/// Puts the unit on disk and hands it to the service manager: what installing means on
/// every platform that keeps a file, and nothing on the one that does not.
///
/// One function because there are two doors to it, and both read `PERCH_HOME` off their
/// own environment, so both owe [`Unit::refuse_what_the_format_cannot_hold`].
fn write_and_start(host: &dyn Host, unit: &Unit, at: Option<&std::path::Path>) -> Result<()> {
    let manager = Manager::of(host);
    unit.refuse_what_the_format_cannot_hold(manager)?;

    // The directory the decision log goes in, before anything is told to write there:
    // Perch's home is made on the way to the first lock, and neither door here takes
    // one.
    if let Some(log) = unit.log.as_deref().and_then(std::path::Path::parent) {
        host.create_private_dir_all(log).map_err(|err| {
            PerchError::file_write(log, format!("could not make room for the log: {err}"))
        })?;
    }

    if let (Some(at), Some(rendered)) = (at, unit.rendered(manager)) {
        if let Some(parent) = at.parent() {
            host.create_dir_all(parent).map_err(|err| {
                PerchError::file_write(parent, format!("could not make room for the unit: {err}"))
            })?;
        }
        crate::host::write_atomically(host, at, &rendered)
            .map_err(|err| PerchError::file_write(at, err))?;
    }

    drive(host, manager.starting(unit, at))
}

/// Writes the unit again against the binary that is there now, after an Upgrade has
/// moved it.
///
/// Answers what to say rather than saying it, and never fails: the binary really is
/// newer, so this is a warning with a one-command repair.
pub fn refreshed_after_an_upgrade(host: &dyn Host) -> Option<String> {
    if !is_there(host) {
        return None;
    }

    let refreshed = describe(host).and_then(|unit| {
        let at = Manager::of(host).unit_path(host)?;
        write_and_start(host, &unit, at.as_deref())?;
        Ok(unit.binary)
    });

    Some(match refreshed {
        Ok(binary) => format!(
            "The Service was restarted, and now runs {}.",
            binary.display(),
        ),
        // A warning with its repair, because the old binary may be gone and the Service
        // may not come up at the next login.
        Err(why) => format!(
            "The Service could not be restarted against the new binary: {why}\n\
             Perch itself upgraded. Run `perch watcher install` to point the \
             Service at it. Until then it may not come up when you log in.",
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
        log: Manager::of(host).log_path(host)?,
        user_id: host.user_id(),
        // Off the machine, because `schtasks` has no notation for "whoever is running
        // this": `%USERNAME%` is `cmd.exe`'s, expanded by a shell that is not there.
        user_name: host.env_var("USERNAME"),
    })
}

/// Whether a Service is installed, asked of whatever this arrangement keeps one in: a
/// file on the two that have one, and the service manager itself on the one that does
/// not.
fn is_installed(host: &dyn Host, at: Option<&std::path::Path>) -> Result<bool> {
    match at {
        Some(at) => Ok(host.path_exists(at)),
        None => {
            Ok(!Manager::of(host).keeps_a_unit_file() && still_held_by_the_service_manager(host))
        }
    }
}

/// The service-manager binary, spelled absolutely on Windows.
///
/// A bare name is searched for in *the current working directory* before `PATH`, and a
/// `schtasks` `/TR` argument is a command line Windows runs at every logon. Windows
/// alone: `systemctl` has no absolute path that holds across distributions.
fn located(host: &dyn Host, program: &str) -> String {
    if host.platform() != Platform::Windows {
        return program.to_string();
    }
    match host.env_var("SystemRoot").filter(|root| !root.is_empty()) {
        Some(root) => format!("{root}\\System32\\{program}.exe"),
        // Said by the failure rather than refused here: every caller already reports
        // what would not run.
        None => program.to_string(),
    }
}

/// What the service manager answers when asked about the Service, or `None`
/// where it would not run at all.
fn asked_of_the_service_manager(host: &dyn Host) -> Option<crate::host::Execution> {
    let asking = Manager::of(host).asking(host.user_id())?;
    let args: Vec<&str> = asking.args.iter().map(String::as_str).collect();
    host.exec(&located(host, &asking.program), &args).ok()
}

/// Whether the service manager says it is running right now.
fn is_running(host: &dyn Host) -> bool {
    asked_of_the_service_manager(host).is_some_and(|ran| Manager::of(host).says_it_is_running(&ran))
}

/// Whether the service manager still holds the Service at all — throttled, waiting
/// to restart, or merely registered.
///
/// The question a Purge has, and the one Windows keeps a Service in: something that
/// can come back and write a Credential is a hazard whether or not it is up now.
fn still_held_by_the_service_manager(host: &dyn Host) -> bool {
    asked_of_the_service_manager(host).is_some_and(|ran| ran.succeeded())
}

/// Whether *a Watcher* is running, which is a different question from whether the
/// Service is.
///
/// Asked of the watcher lock rather than of the process table, which a renamed binary
/// defeats, and read by trying to take it and giving it straight back.
fn watcher_is_running(host: &dyn Host) -> bool {
    let Ok(spec) = holdings::watcher_lock_spec(host) else {
        return false;
    };
    // Asked rather than requested: `take_all` answers this too, but only after
    // exhausting five attempts, and a fault is not contention and is not read as one.
    crate::lock::is_held(host, &spec).unwrap_or(false)
}

/// Where the decision log goes, read back out of the unit that is installed.
///
/// Read rather than recomputed: a value worked out again from the machine would agree
/// with the machine by construction. Falls back to the derivation only where there is
/// no unit file to read it out of.
fn recorded_log(host: &dyn Host, unit: Option<&str>, installed: bool) -> Result<Option<PathBuf>> {
    // `None` where there is nothing installed to write one, as `binary` is
    // `null` beside a `binary_exists` of `null`: a path here is a file a script
    // would tail and nothing would ever append to.
    if !installed {
        return Ok(None);
    }
    let manager = Manager::of(host);
    if !manager.keeps_a_unit_file() {
        return manager.log_path(host);
    }
    Ok(unit.and_then(|text| manager.log_in(text)))
}

/// Whether any Scope has told the Watcher it may act, or `None` where the registry
/// would not load.
///
/// Asked of the whole registry rather than of the active Account, because a Service
/// outlives whichever Account happens to be active when it is installed.
fn any_scope_may_act(host: &dyn Host) -> Option<bool> {
    // Whatever is wrong with a registry that will not load is bigger news than this
    // answer, and the command that needs it will say so.
    let Ok(Some(registry)) = registry::load(host) else {
        return None;
    };

    Some(
        registry
            .scopes()
            .iter()
            .any(|scope| cycle::may_act_within(&registry, scope).may()),
    )
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
            // What the service manager said rather than a code: `systemctl` and
            // `launchctl` both explain themselves, and a sentence naming the unit is
            // worth more than "exited 1".
            Ok(ran) => match ran.stderr.trim().is_empty() {
                true => ran.stdout.trim().to_string(),
                false => ran.stderr.trim().to_string(),
            },
            Err(err) => format!("{err}. Is `{}` on your PATH?", step.program),
        };
        return Err(PerchError::Other(format!(
            "`{}` failed: {failed}",
            step.as_typed(),
        )));
    }
    Ok(())
}

/// Refuses to install a Service for somebody who is not the person it would watch.
///
/// Installed under `sudo` it would be a root process watching root's registry, which is
/// empty, while the person who typed it wondered why nothing ever Switched.
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
