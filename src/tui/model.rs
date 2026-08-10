//! What the TUI is showing, and what a keystroke does to it.
//!
//! Everything the frame loop knows lives here, and nothing here draws or reads:
//! a keystroke goes in and a different [`Model`] comes out, so what the two tabs
//! do about `Tab`, `j` and `r` is tested without a terminal and without a clock.

use chrono::{DateTime, Utc};

use crate::commands::{run, switch};
use crate::cycle::{self, Scope};
use crate::registry::{Account, Registry};
use crate::tui::refresh::Refreshed;
use crate::tui::{Signal, lines_of};

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
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum Asked {
    /// Draw the next frame. Nearly every keystroke.
    Nothing,
    /// Somebody pressed the key that spends network budget.
    ForARefresh,
    /// Somebody pressed the key that makes the Account under the cursor the
    /// active one. The Switch itself is `perch switch`'s
    /// ([`crate::tui::act::switch`]) — everything the model can decide about it
    /// has been decided by the time this comes back, including which Account it
    /// is: the model has just had it in hand to check, so carrying it here saves
    /// the caller a second lookup and the possibility of the two disagreeing.
    ForASwitch(String),
}

/// How the view ended, which is not always "it ended".
///
/// A Run hands the terminal over to a client for as long as somebody's session
/// lasts, so it is not something the frame loop can do and come back from: the
/// view ends, the terminal goes back, and the client is launched into it. A
/// Switch is instant and the picker is still worth looking at afterwards, so
/// that one happens inside the loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Left {
    /// `q` or Ctrl-C. Perch is done.
    Alone,
    /// The Run key, naming the Account to launch a client as.
    ToRun(String),
}

/// One scope's worth of the listing: what it is, and which rows of the listing
/// its Accounts are.
///
/// A Cycle never leaves the scope it started in (ADR 0002), so the listing is
/// one ranking per scope rather than one over everything ([`ranked`]) — and the
/// scope is the only level at which a figure spanning several Accounts means
/// anything at all ([`crate::reserve`]). Carried rather than worked out per
/// frame for the same reason the order is: the row the cursor is on cannot
/// change between deciding it and drawing it.
#[derive(Debug, Clone)]
pub struct Section {
    pub scope: Scope,
    /// Where its Accounts sit in the listing — positions in
    /// [`Model::accounts`], not in the registry.
    pub rows: std::ops::Range<usize>,
}

/// Everything the TUI is showing.
pub struct Model {
    /// The Accounts as Perch last held them: read before the terminal was
    /// entered, and replaced wholesale by a Refresh or a Switch that lands.
    ///
    /// Private, and behind [`Model::now_holds`], because `order` below is
    /// positions into it: the two are one fact in two pieces, and a registry
    /// anybody could assign is an order that would quietly index the wrong
    /// Accounts.
    registry: Registry,
    /// Where each Account sits in the listing — positions into
    /// `registry.accounts`, in the order a Cycle ranks them. Held rather than
    /// worked out per frame so that the row the cursor is on cannot change
    /// between deciding it and drawing it.
    order: Vec<usize>,
    /// Which scope each stretch of the listing belongs to, in the same order.
    /// Cut from the same pass that builds `order`, so a section can never name
    /// rows the listing does not have.
    sections: Vec<Section>,
    /// The clock the ages on the figures are measured against, moved on by the
    /// frame loop rather than read here.
    pub now: DateTime<Utc>,
    pub tab: Tab,
    /// Which Account the cursor is on, as a position in the listing.
    ///
    /// A position rather than an email address, because it is a place on the
    /// screen and an arrow key moves it. What it points *at* survives the
    /// listing being reordered — see [`Model::now_holds`].
    pub cursor: usize,
    pub refreshing: Refreshing,
    /// What the last act said, in the words the command said it in. Cleared
    /// when the cursor moves, because a report standing beside a different
    /// Account is a report about the wrong one.
    pub said: Vec<String>,
    /// What ends the loop, and what the view was left for — `None` for as long
    /// as it is still running. One field rather than a flag beside an Account,
    /// because leaving and what it was left for are one fact and a loop that
    /// could read a half of it would be a Run nobody launched.
    pub leaving: Option<Left>,
}

impl Model {
    pub fn new(registry: Registry, now: DateTime<Utc>) -> Model {
        let (order, sections) = ranked(&registry, now);
        Model {
            order,
            sections,
            registry,
            now,
            tab: Tab::Accounts,
            cursor: 0,
            refreshing: Refreshing::Unasked,
            said: Vec::new(),
            leaving: None,
        }
    }

    /// The Accounts as Perch last held them, for the frame that draws them.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Every Account Perch holds, in the order the listing shows them — which
    /// is the order a Cycle ranks them ([`ranked`]).
    pub fn accounts(&self) -> Vec<&Account> {
        self.order
            .iter()
            .map(|at| &self.registry.accounts[*at])
            .collect()
    }

    /// The listing cut into the scopes it is a ranking within, in the order it
    /// shows them. Every Account is in exactly one, and a scope holding none is
    /// not among them: a heading over no Accounts is a heading that says
    /// nothing.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// The Account under the cursor, or `None` when Perch holds none.
    pub fn selected(&self) -> Option<&Account> {
        self.order
            .get(self.cursor)
            .map(|at| &self.registry.accounts[*at])
    }

    /// Takes a registry that has moved on — a Refresh that landed, a Switch
    /// that did — and keeps the cursor on the Account it was on.
    ///
    /// The listing is ranked, so a figure that came back can reorder it: the
    /// row under the cursor before is not the row under it after. What the two
    /// acting keys act on is the Account rather than the row number, so it
    /// follows the Account — otherwise pressing `r` and then Enter would Switch
    /// to something nobody chose.
    pub fn now_holds(&mut self, registry: Registry) {
        let was_on = self.selected().map(|account| account.email().to_string());
        self.registry = registry;
        (self.order, self.sections) = ranked(&self.registry, self.now);

        let found = was_on.and_then(|email| self.row_of(&email));
        if found.is_none() {
            // The Account the cursor was on is gone — another `perch remove`
            // while this was open, say. The cursor keeps its row number, so it
            // is on a different Account now, and a report left standing beside
            // it would be a report about the wrong one. That is the rule
            // `move_to` exists for; this is the other way the cursor comes to
            // point somewhere new.
            self.said.clear();
        }
        self.cursor = found.unwrap_or(self.cursor).min(self.last_row());
    }

    /// Where an Account sits in the listing now, or `None` if it is no longer
    /// held at all.
    fn row_of(&self, email: &str) -> Option<usize> {
        // Over `order` rather than over `accounts()`, which allocates the whole
        // listing to find one position — and this runs on every landed Refresh
        // and every Switch.
        self.order
            .iter()
            .position(|at| self.registry.accounts[*at].email() == email)
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
            Signal::Leave => self.leaving = Some(Left::Alone),
            Signal::NextTab => self.tab = self.tab.stepped(1),
            Signal::PreviousTab => self.tab = self.tab.stepped(Tab::ALL.len() - 1),
            Signal::Down => self.move_to((self.cursor + 1).min(self.last_row())),
            Signal::Up => self.move_to(self.cursor.saturating_sub(1)),
            // The size is the screen's business rather than the model's: the
            // next frame is drawn against whatever the terminal now is.
            Signal::Resized(_, _) => {}
            Signal::Refresh => return self.ask_for_a_refresh(),
            Signal::Switch => return self.ask_for_a_switch(),
            Signal::Run => self.ask_for_a_run(),
        }
        Asked::Nothing
    }

    /// Moves the cursor, and drops whatever the last act said: the report was
    /// about the Account it was on.
    fn move_to(&mut self, row: usize) {
        if row != self.cursor {
            self.said.clear();
        }
        self.cursor = row;
    }

    /// Enter: make the Account under the cursor the active one.
    ///
    /// Everything that can be decided from what is on screen is decided here,
    /// and the Switch itself is `perch switch`'s. What that leaves is two
    /// refusals: an Account whose Credential is known not to work, in the words
    /// the command would have refused it in, and a Refresh that is still out.
    ///
    /// The second is not politeness. A Refresh holds Perch's own lock while it
    /// writes what it read, and a Switch is taken on the frame loop — so one
    /// started underneath a Refresh would sit waiting on that lock with the
    /// screen frozen, which is the one thing the whole design of this loop is
    /// for avoiding.
    fn ask_for_a_switch(&mut self) -> Asked {
        let Some(account) = self.selected() else {
            return Asked::Nothing;
        };
        let email = account.email().to_string();
        let usable = switch::refuse_a_quarantined_account(&self.registry, account);
        if self.refuses(usable) {
            return Asked::Nothing;
        }
        if self.refreshing == Refreshing::Waiting {
            self.said = vec![WAITING_ON_A_REFRESH.to_string()];
            return Asked::Nothing;
        }
        Asked::ForASwitch(email)
    }

    /// The Run key: give the terminal back and launch a client as the Account
    /// under the cursor.
    ///
    /// The Quarantine is refused here rather than by the Run, because a Run
    /// from the picker gives the screen back before it launches anything: a
    /// refusal arriving after that is one the user reads with the view they
    /// were choosing in already gone.
    fn ask_for_a_run(&mut self) {
        let Some(email) = self.selected().map(|account| account.email().to_string()) else {
            return;
        };
        if self.refuses(run::refuse_a_quarantined_account(&self.registry, &email)) {
            return;
        }
        self.leaving = Some(Left::ToRun(email));
    }

    /// Whether the command the key stands for refused, putting its refusal on
    /// the next frame where it did.
    ///
    /// The refusal is the command's own, word for word — the two acting keys
    /// name an Account by cursor and differ from a typed command in nothing
    /// else, so a sentence written here would be a second opinion about one
    /// state.
    fn refuses(&mut self, asked: crate::error::Result<()>) -> bool {
        match asked {
            Ok(()) => false,
            Err(refused) => {
                self.said = lines_of(&refused.to_string());
                true
            }
        }
    }

    /// A Refresh, unless one is already out.
    ///
    /// Holding the second one is not politeness about the endpoint's budget —
    /// though it is that too — it is that a Refresh writes the registry under
    /// Perch's own lock, and the second would sit waiting on the first for as
    /// long as the first took.
    /// Nothing to read is also nothing to ask for. A Refresh over no Accounts
    /// still takes Perch's own lock exclusively and still has to be waited out
    /// on the way `q` — so on a machine holding nothing, `r` said "Refreshing"
    /// and could block on another `perch` for as long as that one held the
    /// lock, to read nought Accounts.
    fn ask_for_a_refresh(&mut self) -> Asked {
        if self.refreshing == Refreshing::Waiting || self.accounts_on_show().is_empty() {
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
            self.now_holds(registry);
        }
        self.refreshing = Refreshing::Back(refreshed.notes);
    }

    /// The furthest row the cursor may sit on. Zero with no Accounts at all,
    /// which is where it already is.
    fn last_row(&self) -> usize {
        self.order.len().saturating_sub(1)
    }
}

/// Why a Switch was not taken while a Refresh is out.
const WAITING_ON_A_REFRESH: &str = "The Refresh you asked for is still out, and it holds Perch's \
                                    own lock while it writes what it read. Press Enter again once \
                                    it is back.";

/// The Accounts in the order `perch switch` would rank them.
///
/// A Cycle never leaves the scope it started in (ADR 0002), so there is no one
/// ranking over every Account Perch holds: each Group ranks its own, by its own
/// Strategy, and the Accounts in no Group rank among themselves when anything
/// has said they may. The listing is those scopes one after another — which is
/// why the Group is a column beside the Headroom rather than a sort key nobody
/// can see.
///
/// The scope the active Account is in comes first, because it is where you are
/// and, wherever a Cycle happens at all, the one a bare `perch switch` looks in.
///
/// Both tabs are drawn from this one order, because the cursor is shared
/// between them: two orders would be a `Tab` that moved what the acting keys
/// act on.
fn ranked(registry: &Registry, now: DateTime<Utc>) -> (Vec<usize>, Vec<Section>) {
    let mut order = Vec::with_capacity(registry.accounts.len());
    let mut sections = Vec::new();
    for scope in scopes(registry) {
        let from = order.len();
        for account in listed(registry, &scope, now) {
            let at = registry
                .accounts
                .iter()
                .position(|held| held.email() == account.email())
                .expect("the listing is of Accounts the registry holds");
            order.push(at);
        }
        if order.len() > from {
            sections.push(Section {
                scope,
                rows: from..order.len(),
            });
        }
    }
    (order, sections)
}

/// One scope's Accounts: ranked where a Cycle could happen in it, and in the
/// order they were added where one could not.
///
/// Being in no Group is the absence of a declaration that Accounts are
/// interchangeable rather than a weaker form of one (ADR 0017), so until
/// `cycle-ungrouped` says otherwise a bare `perch switch` refuses there instead
/// of choosing. Ordering those Accounts by Headroom would show a ranking Perch
/// would not make — the one thing this listing exists not to do — so they are
/// left as `perch list` shows them, with the Headroom still beside each of them
/// as the figure it is.
fn listed<'a>(registry: &'a Registry, scope: &Scope, now: DateTime<Utc>) -> Vec<&'a Account> {
    match cycle::may_cycle_within(registry, scope) {
        true => cycle::ranked(registry, scope, now),
        false => scope.accounts(registry),
    }
}

/// Every scope, with the one the active Account is in first.
///
/// Every Group and the Accounts in no Group, so an Account is in exactly one of
/// them and the listing holds all of them: an Account that fell out of the
/// order would be one the picker could not reach with the arrow keys.
fn scopes(registry: &Registry) -> Vec<Scope> {
    let mut every: Vec<Scope> = registry
        .group_names()
        .map(|name| Scope::Group(name.to_string()))
        .collect();
    every.push(Scope::Ungrouped);

    let Some(active) = registry.active_account() else {
        return every;
    };
    let here = match &active.group {
        Some(name) => Scope::Group(name.clone()),
        None => Scope::Ungrouped,
    };
    let mut ordered = vec![here.clone()];
    ordered.extend(every.into_iter().filter(|scope| *scope != here));
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::Identity;
    use crate::registry::{CachedUtilization, Quarantine, Registry, WindowUtilization};
    use chrono::TimeZone;

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 7, hour, 0, 0).unwrap()
    }

    fn account(email: &str) -> Account {
        Account {
            identity: Identity {
                email: email.to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            enabled: true,
            quarantine: None,
            group: None,
            utilization: None,
        }
    }

    /// The same Account with one Quota Window that full.
    fn used(mut account: Account, percent: f64) -> Account {
        account.utilization = Some(CachedUtilization {
            observed_at: at(11),
            windows: vec![WindowUtilization {
                window: "5-hour".to_string(),
                used_percent: percent,
                resets_at: None,
            }],
        });
        account
    }

    fn in_group(mut account: Account, group: &str) -> Account {
        account.group = Some(group.to_string());
        account
    }

    fn registry_of(accounts: Vec<Account>) -> Registry {
        let mut registry = Registry::default();
        for account in accounts {
            if let Some(group) = &account.group {
                let _ = registry.declare_group(group);
            }
            registry.upsert(account);
        }
        registry
    }

    fn model_holding(accounts: Vec<Account>) -> Model {
        Model::new(registry_of(accounts), at(12))
    }

    fn model_of(emails: &[&str]) -> Model {
        model_holding(emails.iter().map(|email| account(email)).collect())
    }

    fn shown(model: &Model) -> Vec<&str> {
        model.accounts().into_iter().map(Account::email).collect()
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
        assert_eq!(model.leaving, None);

        let _ = model.act_on(Signal::Leave);
        assert_eq!(model.leaving, Some(Left::Alone));
    }

    /// The listing is what the choice would be, so the Account at the top is
    /// the one a bare `perch switch` would land on.
    #[test]
    fn the_accounts_are_shown_in_the_order_a_cycle_ranks_them() {
        let model = model_holding(vec![
            in_group(used(account("tired@example.com"), 90.0), "work"),
            in_group(used(account("fresh@example.com"), 10.0), "work"),
            in_group(used(account("middling@example.com"), 50.0), "work"),
        ]);

        assert_eq!(
            shown(&model),
            [
                "fresh@example.com",
                "middling@example.com",
                "tired@example.com"
            ]
        );
    }

    /// A Cycle never leaves the scope it started in (ADR 0002), so the listing
    /// is one ranking per scope — and the one a bare `perch switch` would Cycle
    /// within goes first, so the top of the listing is where Perch would go.
    #[test]
    fn the_scope_a_bare_switch_would_cycle_within_is_listed_first() {
        let mut registry = registry_of(vec![
            used(account("loose@example.com"), 1.0),
            in_group(used(account("here@example.com"), 60.0), "work"),
            in_group(used(account("spare@example.com"), 20.0), "work"),
        ]);
        registry.active = Some("here@example.com".to_string());

        let model = Model::new(registry, at(12));

        assert_eq!(
            shown(&model),
            ["spare@example.com", "here@example.com", "loose@example.com",],
            "Group `work` first and ranked, then the Accounts in no Group",
        );
    }

    /// A Refresh can reorder the listing, because the figure it brought back is
    /// what the order is made on. What the acting keys act on is the Account
    /// rather than the row, so pressing `r` and then Enter cannot Switch to
    /// something nobody chose.
    #[test]
    fn the_cursor_follows_the_account_it_was_on_when_the_order_changes() {
        let mut model = model_holding(vec![
            in_group(used(account("first@example.com"), 10.0), "work"),
            in_group(used(account("second@example.com"), 50.0), "work"),
        ]);
        let _ = model.act_on(Signal::Down);
        assert_eq!(
            model.selected().map(Account::email),
            Some("second@example.com")
        );

        model.refreshed(Refreshed {
            registry: Some(registry_of(vec![
                in_group(used(account("first@example.com"), 80.0), "work"),
                in_group(used(account("second@example.com"), 5.0), "work"),
            ])),
            notes: Vec::new(),
        });

        assert_eq!(model.cursor, 0, "it moved to the top of the listing");
        assert_eq!(
            model.selected().map(Account::email),
            Some("second@example.com"),
            "and the cursor went with it",
        );
    }

    /// And where it cannot follow, it drops what was said about where it was.
    ///
    /// An Account can go while the picker is open — another `perch remove` in
    /// a second terminal — and then there is nothing to follow. The cursor
    /// keeps its row number, which is a different Account, so a report left
    /// standing beside it is a report about the wrong one: the rule `move_to`
    /// exists for, reached the other way.
    #[test]
    fn a_report_does_not_outlive_the_account_it_was_about() {
        let mut model = model_holding(vec![
            in_group(used(account("first@example.com"), 10.0), "work"),
            in_group(used(account("second@example.com"), 50.0), "work"),
        ]);
        let _ = model.act_on(Signal::Down);
        model.said = vec!["Switched to second@example.com.".to_string()];

        // Somebody removed it in another terminal while this was open.
        model.refreshed(Refreshed {
            registry: Some(registry_of(vec![in_group(
                used(account("first@example.com"), 10.0),
                "work",
            )])),
            notes: Vec::new(),
        });

        assert_eq!(
            model.selected().map(Account::email),
            Some("first@example.com"),
            "the cursor lands on what is left"
        );
        assert!(
            model.said.is_empty(),
            "and says nothing about it that was said of somebody else: {:?}",
            model.said
        );
    }

    /// The cursor is also clamped, so a listing that shrank under it does not
    /// leave it pointing past the end and the body drawing empty.
    #[test]
    fn a_listing_that_shrank_leaves_the_cursor_inside_it() {
        let mut model = model_holding(vec![
            in_group(used(account("first@example.com"), 10.0), "work"),
            in_group(used(account("second@example.com"), 50.0), "work"),
            in_group(used(account("third@example.com"), 90.0), "work"),
        ]);
        let _ = model.act_on(Signal::Down);
        let _ = model.act_on(Signal::Down);
        assert_eq!(model.cursor, 2);

        model.refreshed(Refreshed {
            registry: Some(registry_of(vec![in_group(
                used(account("first@example.com"), 10.0),
                "work",
            )])),
            notes: Vec::new(),
        });

        assert_eq!(model.cursor, 0);
        assert!(model.selected().is_some(), "and points at something");
    }

    /// A Quarantine is never a statement that the Account is gone, so it is
    /// still listed and still selectable — and choosing it names the one
    /// command that ends it rather than failing obscurely.
    #[test]
    fn choosing_a_quarantined_account_names_the_repair_and_acts_on_nothing() {
        let mut broken = account("broken@example.com");
        broken.quarantine = Some(Quarantine::RenewalRejected);

        for key in [Signal::Switch, Signal::Run] {
            let mut model = model_holding(vec![broken.clone()]);

            assert_eq!(model.act_on(key), Asked::Nothing, "{key:?}");

            let said = model.said.join(" ");
            assert!(said.contains("Quarantined"), "{key:?}: {said}");
            assert!(
                said.contains("perch relogin broken@example.com"),
                "{key:?}: {said}"
            );
            assert_eq!(model.leaving, None, "{key:?} launched nothing");
        }
    }

    /// A Refresh holds Perch's own lock while it writes what it read, and a
    /// Switch is taken on the frame loop: one started underneath a Refresh
    /// would sit waiting on that lock with the screen frozen.
    #[test]
    fn a_switch_waits_for_the_refresh_that_is_holding_perchs_lock() {
        let mut model = model_of(&["one@example.com"]);
        assert_eq!(model.act_on(Signal::Refresh), Asked::ForARefresh);

        assert_eq!(model.act_on(Signal::Switch), Asked::Nothing);
        assert!(model.said.join(" ").contains("lock"), "{:?}", model.said);

        model.refreshed(Refreshed::nothing_read(vec![]));
        assert_eq!(
            model.act_on(Signal::Switch),
            Asked::ForASwitch("one@example.com".to_string()),
            "one that has come back is not one that is out",
        );
    }

    /// A Run lasts as long as somebody's session, so it is not something the
    /// loop takes and comes back from: the view ends with the Account to launch
    /// as, and the terminal is given back before anything is launched into it.
    #[test]
    fn the_run_key_ends_the_view_naming_the_account_to_launch() {
        let mut model = model_of(&["one@example.com", "two@example.com"]);
        let _ = model.act_on(Signal::Down);

        assert_eq!(model.act_on(Signal::Run), Asked::Nothing);

        assert_eq!(
            model.leaving,
            Some(Left::ToRun("two@example.com".to_string()))
        );
    }

    /// A report standing beside a different Account is a report about the wrong
    /// one.
    #[test]
    fn moving_the_cursor_drops_what_the_last_act_said() {
        let mut model = model_of(&["one@example.com", "two@example.com"]);
        model.said = vec!["Switched to one@example.com.".to_string()];

        let _ = model.act_on(Signal::Up);
        assert_eq!(model.said.len(), 1, "the cursor did not move");

        let _ = model.act_on(Signal::Down);
        assert!(model.said.is_empty());
    }

    /// Nothing to act on is nothing done, rather than a row that is not there.
    #[test]
    fn neither_acting_key_does_anything_with_no_accounts_at_all() {
        let mut model = model_of(&[]);

        assert_eq!(model.act_on(Signal::Switch), Asked::Nothing);
        assert_eq!(model.act_on(Signal::Run), Asked::Nothing);
        assert!(model.said.is_empty(), "{:?}", model.said);
        assert_eq!(model.leaving, None);
    }

    /// ADR 0017: being ungrouped is the absence of a declaration that Accounts
    /// are interchangeable, and `perch switch` refuses to Cycle among them
    /// until one is made. A ranking of Accounts Perch would not choose between
    /// is a claim the listing has no business making.
    #[test]
    fn the_accounts_in_no_group_are_not_ranked_until_cycling_may_choose_them() {
        let held = vec![
            used(account("tired@example.com"), 90.0),
            used(account("fresh@example.com"), 10.0),
        ];

        let model = Model::new(registry_of(held.clone()), at(12));
        assert_eq!(
            shown(&model),
            ["tired@example.com", "fresh@example.com"],
            "as held, which is how `perch list` shows them",
        );

        let mut saying_they_are = registry_of(held);
        saying_they_are.global.cycle_ungrouped = true;
        let model = Model::new(saying_they_are, at(12));
        assert_eq!(
            shown(&model),
            ["fresh@example.com", "tired@example.com"],
            "and ranked once something says a Cycle may move between them",
        );
    }
}
