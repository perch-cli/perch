//! What the TUI is showing, and what a keystroke does to it.
//!
//! Everything the frame loop knows lives here, and nothing here draws or reads:
//! a keystroke goes in and a different [`Model`] comes out, so what the two tabs
//! do about `Tab`, `j` and `r` is tested without a terminal and without a clock.

use chrono::{DateTime, Utc};

use crate::registry::{Account, Registry};
use crate::tui::Signal;
use crate::tui::refresh::Refreshed;

/// The two views, in the order they are offered.
///
/// One command's worth of surface split by what is being asked: "which Account
/// do I want" is a different question from "how much is left where", and a
/// single list answering both answers neither well.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// The Accounts, as things to choose between.
    Accounts,
    /// The figures, as evidence.
    Utilization,
}

impl Tab {
    /// Every view, in the order they are shown. The tab bar and the key that
    /// moves between them read this same array, so a view added here appears in
    /// both without either being told about it.
    pub const ALL: [Tab; 2] = [Tab::Accounts, Tab::Utilization];

    /// What the tab bar calls it, which is also the word the issue and the
    /// README use for it.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Accounts => "Accounts",
            Tab::Utilization => "Utilization",
        }
    }

    /// Where it sits in the bar.
    pub fn index(self) -> usize {
        Tab::ALL
            .iter()
            .position(|tab| *tab == self)
            .expect("every Tab is in Tab::ALL")
    }

    /// The next view round, and the one before it. Both wrap: with two tabs
    /// every step is the other one, and a bar that stopped at its ends would
    /// make the second tab harder to reach than the first.
    fn stepped(self, by: usize) -> Tab {
        Tab::ALL[(self.index() + by) % Tab::ALL.len()]
    }
}

/// Where the last Refresh got to — the whole of what the TUI says about the
/// network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refreshing {
    /// Nobody has asked for one. The ordinary state: figures come from cache
    /// (ADR 0015).
    Unasked,
    /// Asked for, and still out. Said on screen, because a keystroke that
    /// appears to do nothing is a keystroke somebody presses again.
    Waiting,
    /// Back, carrying whatever could not be read. Empty when everything was:
    /// the figures then say "just now" themselves, which is the news.
    Back(Vec<String>),
}

/// What the frame loop has to do about a keystroke, where drawing the next
/// frame is not the whole of it.
///
/// A return value rather than a flag on the model, because the loop is the only
/// thing that can act on it and it must not be able to forget: asking is the
/// difference between a Refresh happening and a screen that says it is waiting
/// for one that was never asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum Asked {
    /// Draw the next frame. Nearly every keystroke.
    Nothing,
    /// Somebody pressed the key that spends network budget.
    ForARefresh,
}

/// Everything the TUI is showing.
pub struct Model {
    /// The Accounts as Perch last held them: read before the terminal was
    /// entered, and replaced wholesale by a Refresh that lands.
    pub registry: Registry,
    /// The clock the ages on the figures are measured against, moved on by the
    /// frame loop rather than read here.
    pub now: DateTime<Utc>,
    pub tab: Tab,
    /// Which Account the cursor is on, as a position in the listing.
    ///
    /// A position rather than an email address, because it is a place on the
    /// screen: an arrow key moves it, and it is the one piece of state that a
    /// Refresh — which can Quarantine an Account but never adds or removes one
    /// — leaves exactly where it was.
    pub cursor: usize,
    pub refreshing: Refreshing,
    /// Set by the keystroke that ends the loop, read by the loop.
    pub leaving: bool,
}

impl Model {
    pub fn new(registry: Registry, now: DateTime<Utc>) -> Model {
        Model {
            registry,
            now,
            tab: Tab::Accounts,
            cursor: 0,
            refreshing: Refreshing::Unasked,
            leaving: false,
        }
    }

    /// Every Account Perch holds, which is every Account either tab shows.
    pub fn accounts(&self) -> &[Account] {
        &self.registry.accounts
    }

    /// The Account under the cursor, or `None` when Perch holds none.
    pub fn selected(&self) -> Option<&Account> {
        self.accounts().get(self.cursor)
    }

    /// Which Accounts a Refresh covers: the ones on screen, and no others.
    ///
    /// Every read spends from an hourly budget that does not refill early (ADR
    /// 0015), so this is the same rule `perch status --refresh` follows —
    /// exactly the Accounts about to be shown. Both tabs show all of them.
    pub fn accounts_on_show(&self) -> Vec<String> {
        self.accounts()
            .iter()
            .map(|account| account.email().to_string())
            .collect()
    }

    /// What one keystroke does.
    pub fn act_on(&mut self, signal: Signal) -> Asked {
        match signal {
            Signal::Leave => self.leaving = true,
            Signal::NextTab => self.tab = self.tab.stepped(1),
            Signal::PreviousTab => self.tab = self.tab.stepped(Tab::ALL.len() - 1),
            Signal::Down => self.cursor = (self.cursor + 1).min(self.last_row()),
            Signal::Up => self.cursor = self.cursor.saturating_sub(1),
            // The size is the screen's business rather than the model's: the
            // next frame is drawn against whatever the terminal now is.
            Signal::Resized(_, _) => {}
            Signal::Refresh => return self.ask_for_a_refresh(),
        }
        Asked::Nothing
    }

    /// A Refresh, unless one is already out.
    ///
    /// Holding the second one is not politeness about the endpoint's budget —
    /// though it is that too — it is that a Refresh writes the registry under
    /// Perch's own lock, and the second would sit waiting on the first for as
    /// long as the first took.
    fn ask_for_a_refresh(&mut self) -> Asked {
        if self.refreshing == Refreshing::Waiting {
            return Asked::Nothing;
        }
        self.refreshing = Refreshing::Waiting;
        Asked::ForARefresh
    }

    /// A Refresh that has come back.
    ///
    /// A failed one leaves every figure standing with the age it had (ADR
    /// 0018): what it could not read is said beside them, rather than the
    /// display emptying because Anthropic was busy.
    pub fn refreshed(&mut self, refreshed: Refreshed) {
        if let Some(registry) = refreshed.registry {
            self.registry = registry;
            self.cursor = self.cursor.min(self.last_row());
        }
        self.refreshing = Refreshing::Back(refreshed.notes);
    }

    /// The furthest row the cursor may sit on. Zero with no Accounts at all,
    /// which is where it already is.
    fn last_row(&self) -> usize {
        self.accounts().len().saturating_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::Identity;
    use crate::registry::Registry;
    use chrono::TimeZone;

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 7, hour, 0, 0).unwrap()
    }

    fn model_of(emails: &[&str]) -> Model {
        let mut registry = Registry::default();
        for email in emails {
            registry.upsert(Account {
                identity: Identity {
                    email: (*email).to_string(),
                    account_uuid: None,
                    organization_name: None,
                    organization_uuid: None,
                },
                plan: None,
                enabled: true,
                quarantine: None,
                group: None,
                utilization: None,
            });
        }
        Model::new(registry, at(12))
    }

    #[test]
    fn the_views_are_a_ring_in_both_directions() {
        let mut model = model_of(&[]);
        assert_eq!(model.tab, Tab::Accounts);

        assert_eq!(model.act_on(Signal::NextTab), Asked::Nothing);
        assert_eq!(model.tab, Tab::Utilization);
        assert_eq!(model.act_on(Signal::NextTab), Asked::Nothing);
        assert_eq!(model.tab, Tab::Accounts);

        assert_eq!(model.act_on(Signal::PreviousTab), Asked::Nothing);
        assert_eq!(model.tab, Tab::Utilization);
    }

    #[test]
    fn the_cursor_stops_at_both_ends_of_the_listing() {
        let mut model = model_of(&["one@example.com", "two@example.com"]);

        let _ = model.act_on(Signal::Up);
        assert_eq!(model.cursor, 0, "there is nothing above the first Account");

        let _ = model.act_on(Signal::Down);
        let _ = model.act_on(Signal::Down);
        assert_eq!(model.cursor, 1, "there is nothing below the last one");
    }

    /// With nothing to point at, the cursor is on nothing rather than on a row
    /// that is not there.
    #[test]
    fn a_cursor_with_no_accounts_points_at_none_of_them() {
        let mut model = model_of(&[]);
        let _ = model.act_on(Signal::Down);
        assert_eq!(model.cursor, 0);
        assert!(model.selected().is_none());
    }

    #[test]
    fn only_the_refresh_key_spends_network_budget() {
        let mut model = model_of(&["one@example.com"]);
        for quiet in [Signal::NextTab, Signal::Down, Signal::Up, Signal::Leave] {
            assert_eq!(model.act_on(quiet), Asked::Nothing);
        }
        assert_eq!(model.refreshing, Refreshing::Unasked);

        assert_eq!(model.act_on(Signal::Refresh), Asked::ForARefresh);
        assert_eq!(model.refreshing, Refreshing::Waiting);
    }

    #[test]
    fn a_second_refresh_is_not_asked_for_while_one_is_out() {
        let mut model = model_of(&["one@example.com"]);
        assert_eq!(model.act_on(Signal::Refresh), Asked::ForARefresh);
        assert_eq!(model.act_on(Signal::Refresh), Asked::Nothing);

        model.refreshed(Refreshed::nothing_read(vec![]));
        assert_eq!(
            model.act_on(Signal::Refresh),
            Asked::ForARefresh,
            "one that has come back is not one that is out"
        );
    }

    /// ADR 0018: what could not be read is said, and what was already known is
    /// still shown.
    #[test]
    fn a_refresh_that_read_nothing_leaves_the_accounts_it_had() {
        let mut model = model_of(&["one@example.com"]);
        let _ = model.act_on(Signal::Refresh);

        model.refreshed(Refreshed::nothing_read(vec!["the network is off".into()]));

        assert_eq!(model.accounts().len(), 1);
        assert_eq!(
            model.refreshing,
            Refreshing::Back(vec!["the network is off".to_string()])
        );
    }

    #[test]
    fn only_the_leaving_key_ends_the_loop() {
        let mut model = model_of(&["one@example.com"]);
        let _ = model.act_on(Signal::Refresh);
        let _ = model.act_on(Signal::NextTab);
        assert!(!model.leaving);

        let _ = model.act_on(Signal::Leave);
        assert!(model.leaving);
    }
}
