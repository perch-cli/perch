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
use std::path::PathBuf;

use crate::commands::{say, say_json};
use crate::cycle;
use crate::error::{EXIT_NOTHING_TO_DO, EXIT_OK, PerchError, Result};
use crate::host::{Host, Platform};
use crate::service::{self, Driven, Unit};
use crate::{registry, upgrade};

/// Writes the unit, starts it, and says what it did.
///
/// Idempotent, because re-running it is the documented repair for a unit whose binary
/// has moved (ADR an-upgrade-asks-its-channel), so a second install replaces rather
/// than refusing — and says which it did.
pub fn install(host: &dyn Host, out: &mut dyn Write) -> Result<i32> {
    refuse_as_root(host)?;

    let unit = describe(host)?;
    // Before the machine is asked anything and before anything is written: a value no
    // format can hold is a refusal about the Unit rather than a half-finished install,
    // and `is_installed` below runs `schtasks` on Windows.
    unit.refuse_what_the_format_cannot_hold(host.platform())?;
    let at = service::unit_path(host)?;
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
                 login — `perch watcher status` says what is there now, and \
                 `perch watcher uninstall` takes it away.",
            )));
        }
        if let Some(at) = &at {
            // In `uninstall`'s order, for the reason `service::forgetting` gives:
            // `enable --now` makes the wants-symlink and *then* starts, so the
            // disable has to reach a unit systemd can still resolve.
            let _ = drive(host, service::stopping(host.platform(), host.user_id()));
            let _ = host.remove_file(at);
            let _ = drive(host, service::forgetting(host.platform()));
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

    // Said rather than refused: refusing would make the order two `perch config set`s
    // are typed in matter, and a Service with no grant holds harmlessly and takes over
    // the moment one is given. The only thing missing is somebody knowing that.
    if let Some(missing) = nothing_may_act(host)? {
        say(out, &missing)?;
    }
    Ok(EXIT_OK)
}

/// Stops the Service and takes the unit back.
pub fn uninstall(host: &dyn Host, out: &mut dyn Write) -> Result<i32> {
    let at = service::unit_path(host)?;
    let installed = is_installed(host, at.as_deref())?;

    // Every step is allowed to fail, because every one is "make sure this is not
    // running", so this drives them all and then judges by what is left.
    let _ = drive(host, service::stopping(host.platform(), host.user_id()));

    if let Some(at) = &at
        && host.path_exists(at)
    {
        host.remove_file(at)
            .map_err(|err| PerchError::file_write(at, err))?;
    }
    // After the removal, which is the whole reason it is not part of `stopping`.
    let _ = drive(host, service::forgetting(host.platform()));

    // Asked again rather than reported off the `installed` above: every step
    // above may fail, and a refused `schtasks /Delete` leaves a task that runs
    // at every logon while this says it is gone.
    if is_installed(host, at.as_deref())? {
        return Err(PerchError::Busy(format!(
            "The Service is still installed, so it was not taken back.\n\
             It is {} and something is refusing to unregister it.",
            service::described(host.platform()),
        )));
    }

    match installed {
        true => {
            say(out, "The Service is stopped and its unit is gone.")?;
            Ok(EXIT_OK)
        }
        // The code for a request that was already true: a machine with no Service is
        // the machine an `uninstall` was asked to produce.
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

/// Says what is installed, whether it is running, and whether its binary is there.
///
/// Exit `0` either way, because answering a question is success. It does not read the
/// log: that would mean shelling out to `journalctl` three ways (ADR
/// a-crate-must-not-cost-a-seam).
pub fn status(host: &dyn Host, json: bool, out: &mut dyn Write) -> Result<i32> {
    let platform = host.platform();
    let at = service::unit_path(host)?;
    let installed = is_installed(host, at.as_deref())?;
    let watching = watcher_is_running(host);
    // Windows is asked differently, because `schtasks /Query` answers whether the task
    // *exists*, which is what `is_installed` asks it. The evidence Windows has is the
    // watcher lock.
    let running = match platform {
        Platform::Windows => installed && watching,
        _ => installed && is_running(host),
    };

    // Read off the unit that is actually installed rather than off what one would be
    // written from now, because whether those two have come apart is the question.
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
                "platform": service::arrangement(platform),
                "unit": at.as_ref().map(|at| at.to_string_lossy()),
                "binary": recorded.as_ref().map(|at| at.to_string_lossy()),
                "binary_exists": binary_is_there,
                // The path alone, and `null` where there is no file to open —
                // on systemd there never is. The sentence a person reads is
                // beside it rather than in its place.
                "log": log.as_ref().map(|at| at.to_string_lossy()),
                "log_said": service::log_is_at(platform, log.as_deref()),
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
                     moves the binary. `perch watcher install` writes the unit \
                     again against the one that is.",
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

    // The Service and the Watcher are different questions, and a machine can answer
    // them differently: one installed and stopped, or a `perch watcher run` somebody
    // typed.
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

/// Stops the Service and takes its unit back, before a Purge deletes anything.
///
/// ADR a-removal-lands-first's shape one level up, and it **refuses rather than
/// continuing** where the Service will not stop: a Watcher racing a Purge writes a
/// captured Credential into a Profile directory another process is deleting.
pub fn take_back_before_a_purge(host: &dyn Host, out: &mut dyn Write) -> Result<bool> {
    let at = service::unit_path(host)?;

    // Asked before the Service question rather than after it: a Watcher somebody typed
    // holds the same lock and is the same hazard, on a machine where no Service was
    // ever installed and the early return below is taken.
    if !is_installed(host, at.as_deref())? {
        if watcher_is_running(host) {
            return Err(PerchError::Busy(
                "A Watcher is running, so nothing was purged.\n\
                 It would go on Switching Credentials into Profiles this command \
                 is deleting. Stop it — Ctrl-C in the terminal running \
                 `perch watcher run` — and run this again."
                    .to_string(),
            ));
        }
        return Ok(false);
    }

    let _ = drive(host, service::stopping(host.platform(), host.user_id()));

    // Asked rather than assumed, because every step of `stopping` may fail and this
    // caller judges by what is still running — the watcher lock first, because a unit
    // reads `inactive` while the process it started winds down mid-Switch.
    if watcher_is_running(host) || is_running(host) {
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
    let _ = drive(host, service::forgetting(host.platform()));

    say(out, "The Service is stopped and its unit is gone.")?;
    Ok(true)
}

/// Whether a Service is installed at all, for the commands that have to mention one
/// without managing it.
pub fn is_there(host: &dyn Host) -> bool {
    service::unit_path(host)
        .and_then(|at| is_installed(host, at.as_deref()))
        .unwrap_or(false)
}

/// Puts the unit on disk and hands it to the service manager: what installing means on
/// every platform that keeps a file, and nothing on the one that does not.
///
/// One function because there are two doors to it, and both read `PERCH_HOME` off their
/// own environment, so both owe [`Unit::refuse_what_the_format_cannot_hold`].
fn write_and_start(host: &dyn Host, unit: &Unit, at: Option<&std::path::Path>) -> Result<()> {
    unit.refuse_what_the_format_cannot_hold(host.platform())?;

    // The directory the decision log goes in, before anything is told to write there:
    // Perch's home is made on the way to the first lock, and neither door here takes
    // one.
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
        let at = service::unit_path(host)?;
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
        // Off the machine, because `schtasks` has no notation for "whoever is running
        // this": `%USERNAME%` is `cmd.exe`'s, expanded by a shell that is not there.
        user_name: host.env_var("USERNAME"),
    })
}

/// Whether a Service is installed, asked of whatever this platform keeps one in: a file
/// on the two that have one, and the service manager itself on Windows.
fn is_installed(host: &dyn Host, at: Option<&std::path::Path>) -> Result<bool> {
    match at {
        Some(at) => Ok(host.path_exists(at)),
        None => Ok(host.platform() == Platform::Windows && is_running(host)),
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

/// Whether *a Watcher* is running, which is a different question from whether the
/// Service is.
///
/// Asked of the watcher lock rather than of the process table, which a renamed binary
/// defeats, and read by trying to take it and giving it straight back.
fn watcher_is_running(host: &dyn Host) -> bool {
    let Ok(spec) = registry::watcher_lock_spec(host) else {
        return false;
    };
    // Asked rather than requested: `take_all` answers this too, but only after
    // exhausting five attempts, and a fault is not contention and is not read as one.
    crate::lock::is_held(host, &spec).unwrap_or(false)
}

/// The binary the *installed* unit names, read back out of it.
///
/// Read rather than recomputed: a value worked out again from the machine would agree
/// with the machine by construction. The reading is [`service::binary_in`]'s, and what
/// is left here is the one effect.
fn recorded_binary(host: &dyn Host, at: Option<&std::path::Path>) -> Option<PathBuf> {
    let text = host.read_file(at?).ok()?;
    service::binary_in(host.platform(), &text)
}

/// Where the decision log goes, asked of the unit that is installed.
///
/// [`recorded_binary`]'s rule, applied to the other thing an install bakes in, falling
/// back to the derivation only where there is nothing to read back.
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

/// The line saying a Service will hold because no Scope has granted anything, or `None`
/// where one has.
///
/// Asked of the whole registry rather than of the active Account, because a Service
/// outlives whichever Account happens to be active when it is installed.
fn nothing_may_act(host: &dyn Host) -> Result<Option<String>> {
    // Nothing to say about a registry that will not load: whatever is wrong with it is
    // bigger news than this line, and the command that needs it will say so.
    let Ok(Some(registry)) = registry::load(host) else {
        return Ok(None);
    };

    // Both statements, because a grant alone is not enough for the Ungrouped Accounts
    // (ADR a-group-is-a-declaration): read from `watcher-may-act` alone this line would
    // stay silent.
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
            // What the service manager said rather than a code: `systemctl` and
            // `launchctl` both explain themselves, and a sentence naming the unit is
            // worth more than "exited 1".
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
