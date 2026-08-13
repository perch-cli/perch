//! The things the picker does that are not drawing: a Switch and a write, both
//! taken from the frame loop.
//!
//! A Run is neither, and it is not here: it hands the terminal to a client for
//! as long as somebody's session lasts, so the loop ends with it rather than
//! taking it ([`super::Left`]).
//!
//! The Switch is `perch switch`'s own, called as the command — the Capture
//! first, Claude Code's locks, the Identity patched, the refusal to rewrite
//! Credentials to land where you already are. Not a Switch the TUI assembles
//! out of the same parts: the picker names the Account by cursor and nothing
//! else about it differs, so anything reimplemented here would be a second
//! Switch to keep in step with the first.
//!
//! What the command printed is what the frame shows, line for line. A picker
//! that summarised it would be a third opinion about what just happened to the
//! machine, and the Capture is the half of a Switch worth reading.
//!
//! [`write`] is the same bargain for everything the panel may change (ADR
//! 0034), and for the same reason: `perch config`, `perch alias`, `perch
//! enable` and `perch group` are what run, so the refusals, the ranges and the
//! locking are theirs rather than a second copy kept in step by hand.

use std::collections::BTreeSet;

use crate::commands::switch::{self, SwitchArgs};
use crate::commands::{alias, config, enable, group};
use crate::host::Host;
use crate::registry;
use crate::tui::lines_of;
use crate::tui::model::{Edit, Model};

/// Switches to the Account under the cursor and puts what the command said on
/// the next frame.
///
/// It blocks the loop for as long as the Switch takes, which is what taking
/// Claude Code's locks is worth: nothing else is happening in the terminal, the
/// user asked for exactly this, and a Switch reaches no network. The one wait
/// that could be long — Perch's own lock, held by a Refresh — is refused before
/// it gets here ([`Model::act_on`]).
///
/// The Account comes from the model along with the decision to act, rather than
/// being looked up again here: `ask_for_a_switch` has just had it in hand to ask
/// whether it is Quarantined, so a second lookup could only ever agree or be a
/// bug — and the `else` arm it needed was unreachable, because the model has
/// already answered `Nothing` when there is no Account under the cursor.
pub fn switch(host: &dyn Host, model: &mut Model, email: &str) {
    // What the Host has already remarked on, so what this Switch provokes can
    // be told from it. A remark is made once per process, so anything not in
    // here afterwards belongs to this Switch.
    let before: BTreeSet<String> = host.remarks().into_iter().collect();

    let mut written = Vec::new();
    let ended = switch::run(
        host,
        SwitchArgs {
            target: Some(email.to_string()),
        },
        &mut written,
    );

    let mut said = lines_of(&String::from_utf8_lossy(&written));
    // Including the refusals, which are the interesting half: already there,
    // nowhere to Capture to, a lock somebody else is holding. Each is a
    // sentence the command wrote about a machine the user is looking at.
    if let Err(refused) = ended {
        said.extend(lines_of(&refused.to_string()));
    }
    // And the remarks, which the command does not write and the Host would have
    // put on stderr — which under the picker is the alternate screen, where a
    // line lands in the middle of the display and stays until something redraws
    // over it. `perch tui` has told the Host to keep them for exactly this
    // reason, so a Switch that fell back to a store Perch would rather not have
    // used says so here or nowhere at all.
    said.extend(
        host.remarks()
            .into_iter()
            .filter(|remark| !before.contains(remark)),
    );
    // Re-read rather than patched, because a Switch writes more than which
    // Account is active — a Quarantine it discovered the hard way, for one —
    // and a display that took one and not the other would show an Account as
    // merely idle. Where it cannot be read, what is on screen stands: the
    // Switch's own words are the news, and they have already been kept.
    //
    // Handed to `wrote` rather than applied here, so the ordering that keeps a
    // report from being dropped by its own write is stated once. Inlined, this
    // was the same four lines as `write` below with the two steps the other way
    // round — which is how the bug came to exist in two places at once.
    model.wrote(said, registry::load(host).ok().flatten());
}

/// Writes one change and puts what the command said on the next frame.
///
/// The change is **the command**, not a change the panel assembles out of the
/// same parts: a Setting written here is written by `perch config`, an Alias by
/// `perch alias`, and so on. The panel names a Scope by cursor and a value by
/// arrow key, and differs from a typed command in nothing else — so a write
/// reimplemented here would be a second copy of every refusal to keep in step
/// with the first (ADR 0034).
///
/// That also settles the locking. Each of those commands takes Perch's
/// exclusive lock and gives it straight back, so the panel holds it per edit
/// and never for the life of the screen: an open TUI is not a denial of service
/// against `perch watch`, which takes the same lock every round. The cost is
/// that an edit can be refused — by another `perch` holding the lock, or by
/// `registry::save` refusing to write against a hold that was lost — and the
/// refusal is surfaced rather than swallowed.
pub fn write(host: &dyn Host, model: &mut Model, edit: Edit) {
    let before: BTreeSet<String> = host.remarks().into_iter().collect();

    let mut written = Vec::new();
    let ended = run_it(host, &edit, &mut written);

    let mut said = lines_of(&String::from_utf8_lossy(&written));
    if let Err(refused) = ended {
        said.extend(lines_of(&refused.to_string()));
    }
    said.extend(
        host.remarks()
            .into_iter()
            .filter(|remark| !before.contains(remark)),
    );

    // Re-read rather than patched. A command writes more than the one field the
    // panel named — declaring a Group adds a row to the sidebar, moving an
    // Account empties a column — and where it could not be read, what is on
    // screen stands: the command's own words are the news, and they have
    // already been kept.
    let now_holds = match registry::load(host) {
        Ok(Some(registry)) => Some(registry),
        _ => None,
    };
    model.wrote(said, now_holds);
}

/// The command each change is.
fn run_it(host: &dyn Host, edit: &Edit, out: &mut dyn std::io::Write) -> crate::error::Result<()> {
    match edit {
        // The word count is what says which layer is meant, and it is the same
        // count a person would type: three for a Scope's Override, two for
        // Global's default (`commands::config`).
        Edit::Setting {
            scope,
            key,
            value: Some(value),
        } => {
            let mut words = Vec::new();
            words.extend(scope.word().map(str::to_string));
            words.push(key.as_str().to_string());
            words.push(value.clone());
            config::run(host, config::ConfigCommand::Set { words }, out)
        }
        Edit::Setting {
            scope,
            key,
            value: None,
        } => {
            let mut words = Vec::new();
            words.extend(scope.word().map(str::to_string));
            words.push(key.as_str().to_string());
            config::run(host, config::ConfigCommand::Unset { words }, out)
        }
        Edit::CycleUngrouped(on) => config::run(
            host,
            config::ConfigCommand::Set {
                words: vec!["cycle-ungrouped".to_string(), on.to_string()],
            },
            out,
        ),
        Edit::Alias { email, name } => alias::run(
            host,
            alias::AliasCommand::Set {
                name: name.clone(),
                target: email.clone(),
            },
            out,
        ),
        Edit::Cycling { email, enabled } => enable::run(
            host,
            match enabled {
                true => enable::EnableCommand::Enable {
                    target: email.clone(),
                },
                false => enable::EnableCommand::Disable {
                    target: email.clone(),
                },
            },
            out,
        ),
        Edit::Group { email, group } => group::run(
            host,
            group::GroupCommand::Move {
                target: email.clone(),
                group: group
                    .clone()
                    .unwrap_or_else(|| registry::NO_GROUP.to_string()),
            },
            out,
        ),
        Edit::DeclareGroup(name) => {
            group::run(host, group::GroupCommand::Add { name: name.clone() }, out)
        }
        Edit::RenameGroup { from, to } => group::run(
            host,
            group::GroupCommand::Rename {
                from: from.clone(),
                to: to.clone(),
            },
            out,
        ),
    }
}
