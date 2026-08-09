//! The one thing the picker does that is not drawing: a Switch, taken from the
//! frame loop.
//!
//! It acts on exactly two things, and this is the one of them that happens
//! while the view is still up (ADR 0011). A Run is the other, and it is not
//! here: it hands the terminal to a client for as long as somebody's session
//! lasts, so the loop ends with it rather than taking it ([`super::Left`]).
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

use crate::commands::switch::{self, SwitchArgs};
use crate::host::Host;
use crate::registry;
use crate::tui::lines_of;
use crate::tui::model::Model;

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
    model.said = said;

    // Re-read rather than patched, because a Switch writes more than which
    // Account is active — a Quarantine it discovered the hard way, for one —
    // and a display that took one and not the other would show an Account as
    // merely idle. Where it cannot be read, what is on screen stands: the
    // Switch's own words are the news, and they have already been kept.
    if let Ok(Some(registry)) = registry::load(host) {
        model.now_holds(registry);
    }
}
