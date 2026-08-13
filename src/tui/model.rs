//! What the TUI is showing, and what a keystroke does to it.
//!
//! Everything the frame loop knows lives here, and nothing here draws, reads or
//! writes: a keystroke goes in and a different [`Model`] comes out, so what the
//! two tabs do about `Tab`, `j` and `r` is tested without a terminal and without
//! a clock. Where a keystroke has to reach the disk, this says so by returning
//! an [`Asked`] — the loop is the only thing that can act on it, and a `#[must_use]`
//! is what stops it forgetting.
//!
//! Two tabs, because there are two questions. `Status` answers "where am I and
//! what governs me"; `Config` answers "what does each Scope declare, and let me
//! change it".

use chrono::{DateTime, Utc};

use crate::commands::config::{SETTINGS, Setting, Shape};
use crate::commands::{run, switch};
use crate::cycle;
use crate::registry::{self, Account, Registry, Scope};
use crate::tui::refresh::Refreshed;
use crate::tui::{Signal, lines_of};

/// The two views, in the order they are offered.
///
/// One command's worth of surface split by what is being asked: "where am I"
/// is a different question from "what may I change", and a single page
/// answering both answers neither well.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Where you are: the active Account, the Accounts you could go to, and
    /// the Settings governing you.
    Status,
    /// What each Scope declares, and the keys that change it.
    Config,
}

impl Tab {
    /// Every view, in the order they are shown. The tab bar and the key that
    /// moves between them read this same array, so a view added here appears in
    /// both without either being told about it.
    pub const ALL: [Tab; 2] = [Tab::Status, Tab::Config];

    /// What the tab bar calls it, which is also the word `CONTEXT.md` uses.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Status => "Status",
            Tab::Config => "Config",
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

/// The rows of the `Status` tab's sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusRow {
    /// The active Account, summarised.
    Overview,
    /// The full table, as things to choose between.
    Accounts,
    /// The Settings in force for the active Account's Scope, read-only, each
    /// with where it came from written out.
    Config,
}

impl StatusRow {
    pub const ALL: [StatusRow; 3] = [StatusRow::Overview, StatusRow::Accounts, StatusRow::Config];

    pub fn title(self) -> &'static str {
        match self {
            StatusRow::Overview => "Overview",
            StatusRow::Accounts => "Accounts",
            StatusRow::Config => "Config",
        }
    }
}

/// Which column the keys are acting on.
///
/// Both tabs have a sidebar and something beside it, so both need to say which
/// one `↓` moves — otherwise the page with a table in it swallows the key that
/// reaches the other rows, and the sidebar becomes unreachable once you are on
/// it. `Status` uses two of these; `Config` uses all three, and its middle one
/// is not always there: Global governs every Account there is, so a column of
/// "the Accounts in Global" would be the whole registry listed under a Scope
/// nobody is *in*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    /// Global, Ungrouped, each Group, and the row that declares a new one.
    Scopes,
    /// The Accounts in the Scope the sidebar is on.
    Accounts,
    /// The Scope's Settings, and the facts of the Account beside them.
    Content,
}

/// One row of the sidebar on the `Config` tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeRow {
    Scope(Scope),
    /// Declaring a Group is the one creation the panel offers, because it is
    /// the one that takes nothing away. Deleting one stays out: the Accounts
    /// survive it and become Ungrouped, but that Group's Overrides do not, and
    /// a value nobody can get back is the loss ADR 0034's rule refuses.
    NewGroup,
}

/// One row of the `Config` tab's content pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A Setting, at the Scope the sidebar is on. Editable.
    Setting(Setting),
    /// `cycle-ungrouped`, which is Global's alone and has no per-Scope form —
    /// shown on Global's page because that is where it lives, and on the
    /// Ungrouped page because that is where it takes effect (ADR 0017).
    CycleUngrouped,
    /// The selected Account's Alias. Typed, because a name is the only value
    /// with no natural step.
    Alias,
    /// Whether Cycling may choose the selected Account — the one reversible
    /// per-Account decision, put where the rest of the decisions are.
    Cycling,
    /// Which Group the selected Account is in.
    Group,
    /// Read-only facts about the selected Account.
    Plan,
    Quarantine,
}

impl Row {
    /// What the row is called down the left of the pane.
    pub fn label(&self) -> &str {
        match self {
            Row::Setting(setting) => setting.as_str(),
            Row::CycleUngrouped => "cycle-ungrouped",
            Row::Alias => "alias",
            Row::Cycling => "cycling-may-choose",
            Row::Group => "group",
            Row::Plan => "plan",
            Row::Quarantine => "quarantine",
        }
    }

    /// Whether this row is one of the Scope's Settings, as opposed to a fact
    /// about an Account. The two are laid out together and edited by the same
    /// keys, but only the first has an Override to clear.
    fn is_a_setting(&self) -> bool {
        matches!(self, Row::Setting(_))
    }

    /// Whether the row holds a value with an order the arrows can walk.
    ///
    /// What `←` asks before it decides whether it is stepping a value or
    /// walking back out of the column, which is what `Signal::Left` has always
    /// meant: step where there is something to step, and otherwise one column
    /// left. A name is typed and a plan is a fact, so neither steps — and `→`,
    /// which has nowhere further right to go, is the key that says so.
    fn has_steps(&self) -> bool {
        match self {
            Row::Setting(_) | Row::CycleUngrouped | Row::Cycling | Row::Group => true,
            Row::Alias | Row::Plan | Row::Quarantine => false,
        }
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

/// One change to the registry, as the command that would make it.
///
/// Carried rather than applied, because the model neither reads nor writes.
/// Every one of these is reversible, which is the line ADR 0034 draws: the TUI
/// may write what it can unwrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// A Setting at a Scope. `None` clears the Override so the Scope Inherits
    /// Global again — which is not a state Global itself has.
    Setting {
        scope: Scope,
        key: Setting,
        value: Option<String>,
    },
    /// The one key that is Global's alone.
    CycleUngrouped(bool),
    Alias {
        email: String,
        name: String,
    },
    Cycling {
        email: String,
        enabled: bool,
    },
    /// `None` is out of every Group.
    Group {
        email: String,
        group: Option<String>,
    },
    DeclareGroup(String),
    /// A Group renamed, which keeps its Overrides, its Accounts and the cooldown
    /// the watcher is pacing it by — so this is reversible by renaming it back.
    RenameGroup {
        from: String,
        to: String,
    },
}

/// A change made and not yet written, with how many quiet frames are left
/// before it is.
///
/// **Counted in frames rather than in milliseconds**, so the debounce needs no
/// clock and the fake screen — which already scripts "a wait nobody pressed
/// anything in" — drives it unchanged. A model given a clock would be a model
/// every test had to advance.
///
/// This is a deferred write and not a save button. Nothing has to be
/// remembered and walking away loses nothing: the frames go by on their own,
/// and leaving flushes whatever is left.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    edit: Edit,
    frames_left: usize,
}

/// How many frames with nobody pressing anything a stepped value waits before
/// it is written.
///
/// Holding `→` from 0 to 80 is otherwise sixteen writes and sixteen lock
/// acquisitions, some of which lose the race and leave a half-set value.
const SETTLES_AFTER: usize = 2;

/// Whether a deferred write is about this row, which is what makes replacing it
/// a correction rather than a second change somebody made.
fn about_the_same_row(edit: &Edit, scope: &Scope, key: Setting) -> bool {
    matches!(
        edit,
        Edit::Setting { scope: also, key: same, .. } if also == scope && *same == key
    )
}

/// The same question asked of two edits, for the rows that are not Settings.
///
/// A row is identified by what the edit acts on rather than by where the cursor
/// was: an Account's Group and whether Cycling may choose it are each one row,
/// and `cycle-ungrouped` is one wherever it is shown.
fn about_one_row(one: &Edit, other: &Edit) -> bool {
    match (one, other) {
        (
            Edit::Setting {
                scope, key: held, ..
            },
            _,
        ) => about_the_same_row(other, scope, *held),
        (Edit::CycleUngrouped(_), Edit::CycleUngrouped(_)) => true,
        (Edit::Cycling { email, .. }, Edit::Cycling { email: also, .. })
        | (Edit::Group { email, .. }, Edit::Group { email: also, .. }) => email == also,
        _ => false,
    }
}

/// A name being typed, and what it is for.
///
/// The only place the panel accepts typed input, because a name is the only
/// value with no natural step: a bool has two states, a Strategy has the ones
/// Perch implements, and a percentage has arrows (ADR 0034).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub what: Naming,
    pub typed: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Naming {
    /// A Group to declare.
    NewGroup,
    /// The Group of this name, renamed. A rename keeps everything the Group
    /// carries, which is why it is a rename rather than a Group declared and one
    /// forgotten.
    Group(String),
    /// An Alias for this Account.
    Alias(String),
}

impl Naming {
    /// What is being asked for, above the line being typed into.
    pub fn asks(&self) -> String {
        match self {
            Naming::NewGroup => "Name the new Group:".to_string(),
            Naming::Group(held) => format!("Rename the Group `{held}` to:"),
            Naming::Alias(email) => format!("Name {email}:"),
        }
    }
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
    /// A change to write, as the command that would write it
    /// ([`crate::tui::act::write`]). There is no save button: a change is
    /// written when it is made, and the lock is taken per edit and given
    /// straight back (ADR 0034).
    ToWrite(Edit),
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
    pub scope: cycle::Scope,
    /// Where its Accounts sit in the listing — positions in
    /// [`Model::accounts`], not in the registry.
    pub rows: std::ops::Range<usize>,
}

/// Everything the TUI is showing.
pub struct Model {
    /// The Accounts as Perch last held them: read before the terminal was
    /// entered, and replaced wholesale by a Refresh, a Switch or an edit that
    /// lands.
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
    /// Which row of the `Status` sidebar is showing.
    pub status_row: usize,
    /// Which Account the cursor is on, as a position in the listing.
    ///
    /// A position rather than an email address, because it is a place on the
    /// screen and an arrow key moves it. What it points *at* survives the
    /// listing being reordered — see [`Model::now_holds`].
    pub cursor: usize,
    /// Which Scope the `Config` sidebar is on, as a position in [`Model::scope_rows`].
    pub scope_row: usize,
    /// Which of that Scope's Accounts the middle column is on.
    pub account_row: usize,
    /// Which row of the content pane the cursor is on.
    pub content_row: usize,
    /// Which column the keys are acting on.
    pub column: Column,
    /// A name being typed, where one is.
    pub prompt: Option<Prompt>,
    pending: Option<Pending>,
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
            tab: Tab::Status,
            status_row: 0,
            cursor: 0,
            scope_row: 0,
            account_row: 0,
            content_row: 0,
            column: Column::Scopes,
            prompt: None,
            pending: None,
            refreshing: Refreshing::Unasked,
            said: Vec::new(),
            leaving: None,
        }
    }

    /// The Accounts as Perch last held them, for the frame that draws them.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Whether Perch holds no Account at all.
    ///
    /// A question of its own because both callers were answering it by building
    /// the whole listing and throwing it away: the frame allocated a `Vec` of
    /// every Account to ask `.is_empty()` and then built it again to draw it,
    /// and `ask_for_a_refresh` allocated a `Vec<String>` of every address to
    /// ask the same thing. Four times a second, for as long as the view is
    /// open.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
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

    /// The Account under the cursor on the `Status` tab, or `None` when Perch
    /// holds none.
    pub fn selected(&self) -> Option<&Account> {
        self.order
            .get(self.cursor)
            .map(|at| &self.registry.accounts[*at])
    }

    /// Which row of the `Status` sidebar is showing.
    pub fn status(&self) -> StatusRow {
        StatusRow::ALL[self.status_row.min(StatusRow::ALL.len() - 1)]
    }

    /// The `Config` sidebar, in the order it is offered: Global, the Accounts
    /// in no Group, each Group as it was declared, and the row that declares
    /// another.
    pub fn scope_rows(&self) -> Vec<ScopeRow> {
        let mut rows: Vec<ScopeRow> = self
            .registry
            .scopes()
            .into_iter()
            .map(ScopeRow::Scope)
            .collect();
        rows.push(ScopeRow::NewGroup);
        rows
    }

    /// What the `Config` sidebar's cursor is on — a Scope, or the row that
    /// declares a new Group.
    ///
    /// Named apart from the `scope_row` field it reads, which is the *position*
    /// rather than what sits at it. The two were one word, both public, both in
    /// live code and telling apart only by a pair of brackets.
    pub fn scope_at_the_cursor(&self) -> ScopeRow {
        let rows = self.scope_rows();
        rows[self.scope_row.min(rows.len() - 1)].clone()
    }

    /// The Scope the `Config` tab is showing, or `None` on the row that
    /// declares a new Group — which is not a Scope until it exists.
    pub fn scope(&self) -> Option<Scope> {
        match self.scope_at_the_cursor() {
            ScopeRow::Scope(scope) => Some(scope),
            ScopeRow::NewGroup => None,
        }
    }

    /// The Accounts in the Scope the sidebar is on — the middle column.
    ///
    /// Empty for Global, which governs every Account there is: a column listing
    /// all of them under a Scope nobody is *in* would be the registry wearing a
    /// heading it does not have.
    pub fn scope_accounts(&self) -> Vec<&Account> {
        match self.scope() {
            Some(Scope::Ungrouped) => self.registry.ungrouped_accounts(),
            Some(Scope::Group(name)) => self.registry.accounts_in(&name),
            Some(Scope::Global) | None => Vec::new(),
        }
    }

    /// The Account the middle column is on, where there is one.
    pub fn scope_account(&self) -> Option<&Account> {
        let accounts = self.scope_accounts();
        accounts
            .get(self.account_row.min(accounts.len().saturating_sub(1)))
            .copied()
    }

    /// The rows of the content pane: the Scope's Settings, and then the facts
    /// of the Account the middle column is on.
    pub fn content_rows(&self) -> Vec<Row> {
        let Some(scope) = self.scope() else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        // Where it lives, and where it takes effect. Nowhere else: a Group's
        // page showing it would be a Setting about Accounts that page is not
        // about.
        if scope == Scope::Global || scope == Scope::Ungrouped {
            rows.push(Row::CycleUngrouped);
        }
        rows.extend(SETTINGS.iter().map(|setting| Row::Setting(*setting)));
        if self.scope_account().is_some() {
            rows.extend([
                Row::Alias,
                Row::Cycling,
                Row::Group,
                Row::Plan,
                Row::Quarantine,
            ]);
        }
        rows
    }

    /// The content row the cursor is on.
    pub fn content(&self) -> Option<Row> {
        let rows = self.content_rows();
        rows.get(self.content_row.min(rows.len().saturating_sub(1)))
            .cloned()
    }

    /// The Scope whose Settings govern the active Account — what the `Status`
    /// tab's `Config` row shows, and only that: rules belonging to Scopes
    /// somebody is not in are not rules about them.
    pub fn governing_scope(&self) -> Scope {
        match self.registry.active_account() {
            Some(account) => self.registry.scope_of(account),
            None => Scope::Global,
        }
    }

    /// A Setting as it stands for a Scope: the value on screen, and the Scope
    /// it came from.
    ///
    /// The value is the deferred write where one is waiting, so the row shows
    /// what the arrow keys have done rather than what is still on disk — read
    /// out of the write itself rather than kept beside it, because the two
    /// would otherwise be one fact in two pieces that could disagree. An
    /// unwritten value is shown as coming from the Scope being edited, because
    /// that is what it will be the moment it lands; and where the write is
    /// refused there is nothing pending any more, so the row goes back to what
    /// was actually written.
    pub fn value_of(&self, scope: &Scope, key: Setting) -> (String, Scope) {
        if let Some(pending) = &self.pending
            && about_the_same_row(&pending.edit, scope, key)
            && let Edit::Setting {
                value: Some(waiting),
                ..
            } = &pending.edit
        {
            return (waiting.clone(), scope.clone());
        }
        (
            key.of(&self.registry.in_force(scope)),
            key.source(&self.registry, scope),
        )
    }

    /// What a row reads as right now, and whether the value is Global's rather
    /// than this Scope's own — which is what the dimming says.
    ///
    /// Row-shaped rather than Setting-shaped, which is the whole of it. The
    /// deferred write was overlaid by [`Self::value_of`], and `value_of`
    /// answers about a Setting — so the view, which has to answer for every
    /// row, had no way to ask the pending-aware question about the three that
    /// are not Settings and read them off the registry instead. Six rows of a
    /// Config page moved as the arrow keys were held and three did not, while
    /// [`Self::defer`] promised "the row shows what has been done to it rather
    /// than what is still on disk".
    ///
    /// The Alias row has no overlay because it needs none: a typed name is
    /// written when it is confirmed rather than deferred, so nothing is ever
    /// waiting on that row. Plan and Quarantine are facts rather than Settings
    /// and cannot be edited at all.
    pub fn shown(&self, scope: &Scope, row: &Row) -> (String, bool) {
        match row {
            Row::Setting(setting) => {
                let (value, source) = self.value_of(scope, *setting);
                (value, source == Scope::Global && *scope != Scope::Global)
            }
            // Global's own, wherever it is shown — so on the Ungrouped page it
            // is dimmed like anything else read from Global, and on Global's
            // own page it is not. A write waiting on it is still Global's, so
            // what is pending changes the value and not the dimming.
            Row::CycleUngrouped => {
                let value = match &self.pending {
                    Some(Pending {
                        edit: Edit::CycleUngrouped(waiting),
                        ..
                    }) => *waiting,
                    _ => self.registry.global.cycle_ungrouped,
                };
                (value.to_string(), *scope != Scope::Global)
            }
            _ => {
                let Some(account) = self.scope_account() else {
                    return (String::new(), false);
                };
                let value = match row {
                    Row::Alias => self
                        .registry
                        .alias_of(account.email())
                        .map(str::to_string)
                        .unwrap_or_else(|| "(none)".to_string()),
                    Row::Cycling => match &self.pending {
                        Some(Pending {
                            edit: Edit::Cycling { email, enabled },
                            ..
                        }) if email == account.email() => enabled.to_string(),
                        _ => account.enabled.to_string(),
                    },
                    Row::Group => self
                        .pending_group_of(account.email())
                        .unwrap_or_else(|| account.group.clone())
                        .unwrap_or_else(|| registry::NO_GROUP.to_string()),
                    Row::Plan => account
                        .plan
                        .clone()
                        .unwrap_or_else(|| "(not reported)".to_string()),
                    Row::Quarantine => match account.quarantine {
                        Some(why) => why.as_str().to_string(),
                        None => "none".to_string(),
                    },
                    Row::Setting(_) | Row::CycleUngrouped => unreachable!("answered above"),
                };
                (value, false)
            }
        }
    }

    /// Where a Group write that has not landed yet would put an Account, if one
    /// is waiting.
    ///
    /// Two `Option`s and both mean something: the outer is whether a write is
    /// waiting at all, and the inner is the value it carries — `None` being out
    /// of every Group, which is a place like any other.
    fn pending_group_of(&self, email: &str) -> Option<Option<String>> {
        match &self.pending {
            Some(Pending {
                edit: Edit::Group { email: also, group },
                ..
            }) if also == email => Some(group.clone()),
            _ => None,
        }
    }

    /// Which Accounts a Refresh covers: the ones on screen, and no others.
    ///
    /// Every read spends from an hourly budget that does not refill early (ADR
    /// 0015), so this is the same rule `perch status --refresh` follows —
    /// exactly the Accounts about to be shown.
    pub fn accounts_on_show(&self) -> Vec<String> {
        self.accounts()
            .iter()
            .map(|account| account.email().to_string())
            .collect()
    }

    /// What one keystroke does.
    pub fn act_on(&mut self, signal: Signal) -> Asked {
        // A name being typed swallows the keyboard, because every letter on it
        // is part of the name rather than a command.
        if self.prompt.is_some() {
            return self.typing(signal);
        }
        // And with nothing being typed into, a letter is whatever the key
        // vocabulary says it is ([`Signal::meant_by`]) — or nothing, which is
        // most of them.
        let signal = match signal {
            Signal::Typed(letter) => match Signal::meant_by(letter) {
                Some(signal) => signal,
                None => return Asked::Nothing,
            },
            other => other,
        };
        match signal {
            Signal::Leave => self.leaving = Some(Left::Alone),
            Signal::NextTab => self.moved_to_tab(self.tab.stepped(1)),
            Signal::PreviousTab => self.moved_to_tab(self.tab.stepped(Tab::ALL.len() - 1)),
            Signal::Down => self.moved(1),
            Signal::Up => self.moved(-1),
            Signal::Left => return self.leftwards(),
            Signal::Right => return self.rightwards(),
            Signal::Flip => return self.flipped(),
            Signal::Clear => return self.cleared(),
            Signal::Least => return self.jumped_to_an_end(false),
            Signal::Most => return self.jumped_to_an_end(true),
            Signal::Name => self.asked_for_a_name(),
            // The size is the screen's business rather than the model's: the
            // next frame is drawn against whatever the terminal now is.
            Signal::Resized(_, _) => {}
            Signal::Refresh => return self.ask_for_a_refresh(),
            Signal::Switch => return self.ask_for_a_switch(),
            Signal::Run => self.ask_for_a_run(),
            // Typed only ever means something with a prompt open.
            Signal::Typed(_) | Signal::Backspace => {}
        }
        Asked::Nothing
    }

    /// One frame in which nobody pressed anything, which is what the debounce
    /// is counted in.
    ///
    /// Returns the deferred write when it has settled. A run of these between
    /// two steps is what makes one write rather than several.
    pub fn settled(&mut self) -> Option<Edit> {
        // Nothing settles under an outstanding Refresh. That Refresh holds
        // Perch's own lock while it writes what it read, and a write taken on
        // the frame loop would sit waiting on it with the screen frozen — the
        // one wait this whole design exists to avoid. The edit is held rather
        // than dropped, so it lands the moment the Refresh is back.
        if self.refreshing == Refreshing::Waiting {
            return None;
        }
        let pending = self.pending.as_mut()?;
        if pending.frames_left > 0 {
            pending.frames_left -= 1;
            return None;
        }
        self.pending.take().map(|pending| pending.edit)
    }

    /// Whatever has not been written yet, for the way out: walking away from a
    /// half-made adjustment loses nothing.
    pub fn take_pending(&mut self) -> Option<Edit> {
        self.pending.take().map(|pending| pending.edit)
    }

    /// What a command the panel ran said, and the registry it left behind.
    ///
    /// Nothing is pending by this point either way, so the row now reads from
    /// the registry: where the write landed that is the new value, and where it
    /// was refused it is what was actually written — because a value nobody
    /// wrote is not one anybody should be reading.
    pub fn wrote(&mut self, said: Vec<String>, registry: Option<Registry>) {
        self.said = said;
        if let Some(registry) = registry {
            self.now_holds(registry);
        }
    }

    /// Moves between tabs, and takes what was said with it: a report about the
    /// Account under a cursor on another tab is not about anything here.
    ///
    /// The keys land on the sidebar, which is where a tab opens: arriving
    /// already inside a column would make the row you are on depend on which
    /// tab you came from.
    fn moved_to_tab(&mut self, tab: Tab) {
        if tab != self.tab {
            self.said.clear();
            self.column = Column::Scopes;
        }
        self.tab = tab;
    }

    /// Down or up, in whichever column the keys are acting on.
    fn moved(&mut self, by: i32) {
        match self.tab {
            Tab::Status => match self.column {
                Column::Content => self.cursor = self.moving(self.cursor, by, self.order.len()),
                _ => self.status_row = self.moving(self.status_row, by, StatusRow::ALL.len()),
            },
            Tab::Config => match self.column {
                Column::Scopes => {
                    let was = self.scope_row;
                    self.scope_row = self.moving(was, by, self.scope_rows().len());
                    if self.scope_row != was {
                        // A different Scope is a different set of Accounts and
                        // a different set of rows, so neither cursor below
                        // means what it did.
                        self.account_row = 0;
                        self.content_row = 0;
                    }
                }
                Column::Accounts => {
                    let rows = self.scope_accounts().len();
                    self.account_row = self.moving(self.account_row, by, rows);
                }
                Column::Content => {
                    let rows = self.content_rows().len();
                    self.content_row = self.moving(self.content_row, by, rows);
                }
            },
        }
    }

    /// One position along a column, dropping whatever the last act said if it
    /// went anywhere.
    ///
    /// The dropping is the point, and is why every cursor moves through here: a
    /// report is about the row it was made on, and one left standing beside a
    /// different row is a report about the wrong thing.
    fn moving(&mut self, at: usize, by: i32, rows: usize) -> usize {
        let moved = step(at, by, rows);
        if moved != at {
            self.said.clear();
        }
        moved
    }

    /// Left: step the value down where the cursor is on something with a
    /// natural order, and otherwise move out to the column on the left.
    fn leftwards(&mut self) -> Asked {
        // Stepping was returned unconditionally, which made the arm below
        // unreachable: `←` never left the content column at all, and the only
        // way back to the sidebar was `Tab`, which resets the column as a side
        // effect of changing view. On the `+ new Group` row it was worse —
        // there are no rows to step there, so `←` moved nothing and said
        // nothing, which is the silent key this panel's own rule refuses.
        if self.tab == Tab::Config
            && self.column == Column::Content
            && self.content().is_some_and(|row| row.has_steps())
        {
            return self.stepped(-1);
        }
        self.column = match self.column {
            // Past the middle column only where there is one. Global governs
            // every Account and lists none of its own, and `rightwards` skips
            // it on the way in for the same reason.
            Column::Content if self.tab == Tab::Config && !self.scope_accounts().is_empty() => {
                Column::Accounts
            }
            _ => Column::Scopes,
        };
        Asked::Nothing
    }

    /// Right: step the value up, or move into the column on the right.
    fn rightwards(&mut self) -> Asked {
        if self.tab == Tab::Config && self.column == Column::Content {
            return self.stepped(1);
        }
        self.column = match (self.tab, self.column) {
            // `Status` has no middle column, so one step in is the page itself.
            (Tab::Status, _) => Column::Content,
            (Tab::Config, Column::Scopes) if self.scope_accounts().is_empty() => Column::Content,
            (Tab::Config, Column::Scopes) => Column::Accounts,
            (Tab::Config, _) => Column::Content,
        };
        Asked::Nothing
    }

    /// The Scope and the row the editing keys act on, or `None` where they are
    /// not on one.
    ///
    /// Asked in one place because all four editing keys ask it, and four copies
    /// of "am I on the Config tab, in the content column, on a row that exists"
    /// is three chances for one of them to answer differently.
    fn editing(&self) -> Option<(Scope, Row)> {
        if self.tab != Tab::Config || self.column != Column::Content {
            return None;
        }
        Some((self.scope()?, self.content()?))
    }

    /// One step of whatever the cursor is on, deferred rather than written.
    fn stepped(&mut self, by: i32) -> Asked {
        let Some((scope, row)) = self.editing() else {
            return Asked::Nothing;
        };
        match row {
            Row::Setting(key) => {
                let (value, _) = self.value_of(&scope, key);
                match next_value(&key.shape(), &value, by) {
                    Some(next) => self.defer(scope, key, next),
                    None => Asked::Nothing,
                }
            }
            // The direction, for the reason `next_value` gives about the other
            // bools: these two read the value and inverted it whichever arrow
            // was pressed, so `←` and `→` did the same thing and holding either
            // one landed wherever the repeat count's parity left it.
            Row::CycleUngrouped => self.defer_edit(Edit::CycleUngrouped(by > 0)),
            Row::Cycling => match self.scope_account() {
                Some(account) => {
                    let edit = Edit::Cycling {
                        email: account.email().to_string(),
                        enabled: by > 0,
                    };
                    self.defer_edit(edit)
                }
                None => Asked::Nothing,
            },
            Row::Group => self.step_group(by),
            Row::Alias => {
                self.said = vec![A_NAME_IS_TYPED.to_string()];
                Asked::Nothing
            }
            Row::Plan | Row::Quarantine => {
                self.said = vec![A_FACT_RATHER_THAN_A_SETTING.to_string()];
                Asked::Nothing
            }
        }
    }

    /// Moves the selected Account between Groups, which is a value with a
    /// natural order: out of every Group, then each Group as it was declared.
    fn step_group(&mut self, by: i32) -> Asked {
        let Some(account) = self.scope_account() else {
            return Asked::Nothing;
        };
        let email = account.email().to_string();
        // Where the step before this one is about to put it, where one is
        // waiting. The write is deferred, so the registry still says what it
        // said before the key was pressed — and recomputing from that would
        // have every press in a run land on the same neighbour, which is a list
        // the arrows could not walk more than one place along.
        let held = self
            .pending_group_of(&email)
            .unwrap_or_else(|| account.group.clone());
        let mut places: Vec<Option<String>> = vec![None];
        places.extend(
            self.registry
                .group_names()
                .map(|name| Some(name.to_string())),
        );
        let at = places
            .iter()
            .position(|place| place.as_deref() == held.as_deref())
            .unwrap_or(0);
        let group = places[step(at, by, places.len())].clone();
        if group == held {
            return Asked::Nothing;
        }
        self.defer_edit(Edit::Group { email, group })
    }

    /// Space: flip the simplest Setting there is, at once.
    ///
    /// Not deferred, because a bool has two states — there is no run of
    /// keystrokes to collapse, and a flip that took half a second to appear
    /// would read as a key that did nothing.
    fn flipped(&mut self) -> Asked {
        let Some((scope, row)) = self.editing() else {
            return Asked::Nothing;
        };
        match row {
            Row::Setting(key) => {
                let (value, _) = self.value_of(&scope, key);
                match key.shape() {
                    Shape::YesOrNo | Shape::OneOf(_) => {
                        // The other one, which is what this key is named for.
                        // Asked of `next_value` with a fixed direction it would
                        // set the same value every time, now that a direction
                        // means one — so the flip is spelled out here and the
                        // arrows are left to mean what they point at.
                        let next = match key.shape() {
                            Shape::YesOrNo => (value != "true").to_string(),
                            _ => {
                                let Some(next) = next_value(&key.shape(), &value, 1) else {
                                    return Asked::Nothing;
                                };
                                next
                            }
                        };
                        self.write(Edit::Setting {
                            scope,
                            key,
                            value: Some(next),
                        })
                    }
                    Shape::Range { .. } => {
                        self.said = vec![A_NUMBER_IS_STEPPED.to_string()];
                        Asked::Nothing
                    }
                }
            }
            // The other two bools on the page, flipped rather than stepped for
            // the same reason: `stepped` now takes its answer from the
            // direction, and this key has no direction to take one from.
            //
            // Flipped off what is *shown*, the way `Row::Setting` above reads
            // through `value_of` — and for the same reason, which these two
            // were left out of. A `→` a moment ago defers a write and the row
            // already displays what it will become, so flipping the value on
            // disk sets it to what is on screen already; `write` then drops the
            // pending edit as same-row, and the row reads the same before and
            // after. That is the keystroke-that-did-nothing `write` was written
            // to cure, cured on one of the three bool rows and not on the other
            // two.
            Row::CycleUngrouped => {
                let displayed = self.shown(&scope, &Row::CycleUngrouped).0;
                self.write(Edit::CycleUngrouped(displayed != "true"))
            }
            Row::Cycling => match self
                .scope_account()
                .map(|account| account.email().to_string())
            {
                Some(email) => {
                    let displayed = self.shown(&scope, &Row::Cycling).0;
                    let edit = Edit::Cycling {
                        email,
                        enabled: displayed != "true",
                    };
                    self.write(edit)
                }
                None => Asked::Nothing,
            },
            _ => self.stepped(1),
        }
    }

    /// The key that clears an Override, so the Scope Inherits Global again.
    ///
    /// At Global it does nothing and says why. Global is the value that applies
    /// where nothing narrower is said, so it has nothing above it to Inherit
    /// from — and a key that silently did nothing would leave somebody
    /// wondering whether it had.
    fn cleared(&mut self) -> Asked {
        let Some((scope, row)) = self.editing() else {
            return Asked::Nothing;
        };
        if row == Row::CycleUngrouped {
            self.said = lines_of(GLOBALS_ALONE);
            return Asked::Nothing;
        }
        if !row.is_a_setting() {
            self.said = vec![ONLY_A_SETTING_IS_INHERITED.to_string()];
            return Asked::Nothing;
        }
        if scope == Scope::Global {
            self.said = lines_of(NOTHING_ABOVE_GLOBAL);
            return Asked::Nothing;
        }
        let Row::Setting(key) = row else {
            return Asked::Nothing;
        };
        self.write(Edit::Setting {
            scope,
            key,
            value: None,
        })
    }

    /// Home and End: the ends of a range, one keystroke away.
    fn jumped_to_an_end(&mut self, most: bool) -> Asked {
        let Some((scope, Row::Setting(key))) = self.editing() else {
            return Asked::Nothing;
        };
        let Shape::Range {
            least, most: top, ..
        } = key.shape()
        else {
            return Asked::Nothing;
        };
        let value = match most {
            true => top,
            false => least,
        }
        .to_string();
        self.defer(scope, key, value)
    }

    /// The key that opens the one text field there is.
    fn asked_for_a_name(&mut self) {
        if self.tab != Tab::Config {
            return;
        }
        match (self.scope_at_the_cursor(), self.content(), self.column) {
            (ScopeRow::NewGroup, _, _) => {
                self.prompt = Some(Prompt {
                    what: Naming::NewGroup,
                    typed: String::new(),
                })
            }
            // With the name already in the field, because a rename is nearly
            // always a correction: four characters changed rather than a name
            // typed again from nothing.
            (ScopeRow::Scope(Scope::Group(held)), _, Column::Scopes) => {
                self.prompt = Some(Prompt {
                    what: Naming::Group(held.clone()),
                    typed: held,
                })
            }
            (ScopeRow::Scope(Scope::Global | Scope::Ungrouped), _, Column::Scopes) => {
                self.said = vec![NEITHER_IS_A_GROUP.to_string()]
            }
            (_, Some(Row::Alias), Column::Content) => {
                if let Some(account) = self.scope_account() {
                    self.prompt = Some(Prompt {
                        what: Naming::Alias(account.email().to_string()),
                        typed: String::new(),
                    })
                }
            }
            _ => self.said = vec![NOTHING_HERE_IS_NAMED.to_string()],
        }
    }

    /// What a keystroke does while a name is being typed.
    fn typing(&mut self, signal: Signal) -> Asked {
        let Some(prompt) = self.prompt.as_mut() else {
            return Asked::Nothing;
        };
        match signal {
            Signal::Typed(letter) => {
                prompt.typed.push(letter);
                Asked::Nothing
            }
            Signal::Backspace => {
                prompt.typed.pop();
                Asked::Nothing
            }
            // Esc: the way out of the field, which writes nothing. The key
            // that leaves the whole view is not offered here, because with a
            // field open `q` is the letter.
            Signal::Clear => {
                self.prompt = None;
                Asked::Nothing
            }
            Signal::Switch => self.confirmed(),
            // Ctrl-C, which is not the key `q` is. Raw mode is what makes it a
            // keystroke rather than a signal, so nothing else in Perch catches
            // it and dropping it here left the view with no way out at all: the
            // field took every key, and Esc-then-`q` was the only escape. A
            // screen that does not leave when Ctrl-C is pressed is the one thing
            // the frame loop is written not to be. The name is abandoned, which
            // is what leaving without confirming means.
            Signal::Leave => {
                self.prompt = None;
                self.leaving = Some(Left::Alone);
                Asked::Nothing
            }
            _ => Asked::Nothing,
        }
    }

    /// Enter, on a name being typed.
    ///
    /// The name is checked against the shared Alias/Group namespace here rather
    /// than left to the command, so a collision is refused before anything is
    /// written and the field stays open with what was typed still in it. The
    /// command checks it again, because the registry is the boundary and this
    /// is only a screen.
    fn confirmed(&mut self) -> Asked {
        let Some(prompt) = self.prompt.clone() else {
            return Asked::Nothing;
        };
        let name = prompt.typed.trim().to_string();
        // The same question the command will ask of the real registry a moment
        // later, asked of this one — so the panel cannot come to accept a name
        // the command refuses, or refuse one it would have taken. One function
        // rather than the three primitives reassembled, which is what this was:
        // the shape, then a waiver, then the namespace, in an order the caller
        // had to know.
        //
        // Asked rather than performed. It used to `clone()` the whole Registry
        // twice per keypress to reach a check only a mutator exposed.
        let taken = match &prompt.what {
            Naming::NewGroup => self.registry.refuse_a_name_nothing_may_answer_to(
                registry::NameKind::Group,
                &name,
                None,
            ),
            // Against the Group *this registry* holds rather than the name the
            // field was opened with, because a Refresh landing replaces the
            // registry under an open prompt: by now another `perch` may have
            // renamed or forgotten it. Where it is gone there is nothing here to
            // ask, and the command's own refusal is the one worth showing.
            Naming::Group(held) => match self.registry.declared_group(held) {
                Some(declared) => self.registry.refuse_a_name_nothing_may_answer_to(
                    registry::NameKind::Group,
                    &name,
                    Some(declared),
                ),
                None => Ok(()),
            },
            Naming::Alias(email) => self.registry.refuse_a_name_nothing_may_answer_to(
                registry::NameKind::Alias,
                &name,
                self.registry.alias_of(email),
            ),
        };
        if let Err(refused) = taken {
            self.said = lines_of(&refused.to_string());
            return Asked::Nothing;
        }

        self.prompt = None;
        match prompt.what {
            Naming::NewGroup => self.write(Edit::DeclareGroup(name)),
            Naming::Group(held) => self.write(Edit::RenameGroup {
                from: held,
                to: name,
            }),
            Naming::Alias(email) => self.write(Edit::Alias { email, name }),
        }
    }

    /// A change to write at once, unless something is in the way.
    ///
    /// A write taken now supersedes a deferred one about the same row, and has
    /// to say so: the deferred edit is still sitting in `pending`, and
    /// [`Model::settled`] would write it a couple of frames later — after this
    /// one has already landed. Last write wins, and the later write is the one
    /// the user asked for first.
    ///
    /// So Esc on a threshold stepped a moment ago cleared the Override and then
    /// put it straight back, and Space on a flag stepped a moment ago flipped it
    /// and then unflipped it. Both read as the keystroke being ignored, and
    /// neither left anything on screen to say why.
    ///
    /// Only the same row. A deferred edit about a *different* one is not in
    /// conflict with this write and settles on its own, which is the whole point
    /// of the rule `defer_edit` keeps.
    fn write(&mut self, edit: Edit) -> Asked {
        if let Some(refused) = self.in_the_way() {
            self.said = vec![refused];
            return Asked::Nothing;
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| about_one_row(&pending.edit, &edit))
        {
            self.pending = None;
        }
        Asked::ToWrite(edit)
    }

    /// A change to write once the arrow keys have stopped.
    ///
    /// Kept on screen while it waits, so the row shows what has been done to it
    /// rather than what is still on disk.
    ///
    /// One deferred write at a time, and displacing one about a *different* row
    /// writes it rather than dropping it. Stepping a threshold, moving down a
    /// row and stepping a margin inside half a second is two changes somebody
    /// made, and a deferred write that quietly lost the first would be exactly
    /// the save button this exists not to be.
    fn defer(&mut self, scope: Scope, key: Setting, value: String) -> Asked {
        self.defer_edit(Edit::Setting {
            scope,
            key,
            value: Some(value),
        })
    }

    /// The same, for the rows that are not Settings.
    ///
    /// Every row the arrows step goes through here. The three per-Account ones
    /// wrote at once instead, so holding a key was one registry write and one
    /// lock taken and given back per repeat the terminal sent — the cost ADR
    /// 0034 names and this debounce exists for. It also moved the cursor under
    /// the user: stepping the `group` row takes the Account's rows off the
    /// page, and the repeat that followed landed on whatever Setting was last.
    fn defer_edit(&mut self, edit: Edit) -> Asked {
        if let Some(refused) = self.in_the_way() {
            self.said = vec![refused];
            return Asked::Nothing;
        }
        let displaced = self.pending.replace(Pending {
            edit: edit.clone(),
            frames_left: SETTLES_AFTER,
        });
        match displaced.map(|pending| pending.edit) {
            Some(was) if !about_one_row(&was, &edit) => Asked::ToWrite(was),
            _ => Asked::Nothing,
        }
    }

    /// Why an edit cannot be taken now, where it cannot.
    ///
    /// One reason, and it is the Refresh: it holds Perch's own lock while it
    /// writes what it read, and an edit started underneath it would sit waiting
    /// on that lock with the screen frozen. The same rule a Switch follows, in
    /// the same words, because it is the same lock and the same wait.
    fn in_the_way(&self) -> Option<String> {
        (self.refreshing == Refreshing::Waiting).then(|| WAITING_ON_A_REFRESH.to_string())
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
        // Enter is how you move into the column on the right wherever the keys
        // are still on a sidebar — which is what "yes, that one" means there.
        // On the Config tab it is only ever that: nothing on it is a Switch, and
        // a Switch a keystroke away from an edit would be a Credential moved by
        // somebody adjusting a percentage.
        if self.tab == Tab::Config || self.column != Column::Content {
            return self.rightwards();
        }
        if self.status() != StatusRow::Accounts {
            return Asked::Nothing;
        }
        let Some(account) = self.selected() else {
            return Asked::Nothing;
        };
        let email = account.email().to_string();
        let usable = switch::refuse_a_quarantined_account(&self.registry, account);
        if self.refuses(usable) {
            return Asked::Nothing;
        }
        if let Some(refused) = self.in_the_way() {
            self.said = vec![refused];
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
        // The same three questions `ask_for_a_switch` asks, and the column is
        // the one this was missing. The two acting keys ADR 0011 confines the
        // picker to disagreed about the same state: with the sidebar cursor on
        // the `Accounts` row but the keys still in the sidebar, `Enter` moved
        // right while `x` ended the loop and handed the terminal to a client.
        //
        // Of the two that is the one to get wrong. `x` was chosen over `Enter`
        // precisely because a mistyped Run is expensive, and the frame did not
        // say which column the keys were in until it was made to.
        if self.tab != Tab::Status
            || self.column != Column::Content
            || self.status() != StatusRow::Accounts
        {
            return;
        }
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
    ///
    /// Nothing to read is also nothing to ask for. A Refresh over no Accounts
    /// still takes Perch's own lock exclusively and still has to be waited out
    /// on the way `q` — so on a machine holding nothing, `r` said "Refreshing"
    /// and could block on another `perch` for as long as that one held the
    /// lock, to read nought Accounts.
    fn ask_for_a_refresh(&mut self) -> Asked {
        if self.refreshing == Refreshing::Waiting || self.is_empty() {
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

    /// Takes a registry that has moved on — a Refresh that landed, a Switch
    /// that did, an edit that was written — and keeps every cursor on what it
    /// was on.
    ///
    /// The listing is ranked, so a figure that came back can reorder it: the
    /// row under the cursor before is not the row under it after. What the two
    /// acting keys act on is the Account rather than the row number, so it
    /// follows the Account — otherwise pressing `r` and then Enter would Switch
    /// to something nobody chose.
    pub fn now_holds(&mut self, registry: Registry) {
        let was_on = self.selected().map(|account| account.email().to_string());
        // The Config tab's cursors are followed the same way and for the same
        // reason: an edit can move an Account into another Group, which takes it
        // out of the column it was selected in.
        let scope_was = self.scope_at_the_cursor();
        let account_was = self
            .scope_account()
            .map(|account| account.email().to_string());
        let content_was = self.content();

        self.registry = registry;
        (self.order, self.sections) = ranked(&self.registry, self.now);

        let found = was_on.and_then(|email| self.row_of(&email));
        if found.is_none() {
            // The Account the cursor was on is gone — another `perch remove`
            // while this was open, say. The cursor keeps its row number, so it
            // is on a different Account now, and a report left standing beside
            // it would be a report about the wrong one. That is the rule
            // `moving` exists for; this is the other way the cursor
            // comes to point somewhere new.
            self.said.clear();
        }
        self.cursor = found.unwrap_or(self.cursor).min(self.last_row());

        let rows = self.scope_rows();
        self.scope_row = rows
            .iter()
            .position(|row| *row == scope_was)
            .unwrap_or(self.scope_row)
            .min(rows.len() - 1);
        let accounts = self.scope_accounts();
        self.account_row = account_was
            .and_then(|email| accounts.iter().position(|account| account.email() == email))
            .unwrap_or(self.account_row)
            .min(accounts.len().saturating_sub(1));
        // By identity, like the three above it. Clamped by number alone, this
        // was the one cursor that could be moved under the user by their own
        // keystroke: stepping the `group` row moves the Account out of the
        // Scope, its five rows go with it, and the cursor landed on whatever
        // Setting happened to be last. The write and the redraw are the same
        // iteration, so the highlight never appears to jump — and the next
        // repeat of a held arrow stepped that Setting instead.
        let content = self.content_rows();
        let followed = content_was
            .as_ref()
            .and_then(|was| content.iter().position(|now| now == was));
        if content_was.is_some() && followed.is_none() {
            // A report standing beside a row it is not about, which is the rule
            // `moving` exists for, reached the other way.
            self.said.clear();
        }
        self.content_row = followed
            .unwrap_or(self.content_row)
            .min(content.len().saturating_sub(1));
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

    /// The furthest row the cursor may sit on. Zero with no Accounts at all,
    /// which is where it already is.
    fn last_row(&self) -> usize {
        self.order.len().saturating_sub(1)
    }
}

/// One position up or down a list, stopping at both ends.
fn step(at: usize, by: i32, len: usize) -> usize {
    let last = len.saturating_sub(1);
    match by {
        by if by < 0 => at.saturating_sub(by.unsigned_abs() as usize),
        by => (at + by as usize).min(last),
    }
}

/// The value one step along from this one, or `None` where there is nowhere to
/// step to.
///
/// The steps are the Setting's own ([`Shape`]) rather than the view's, so the
/// panel cannot offer a value `perch config set` would refuse.
fn next_value(shape: &Shape, from: &str, by: i32) -> Option<String> {
    match shape {
        // The direction, not the opposite of what is there. A bool has two
        // states and the arrows have two directions, so `←` is the low one and
        // `→` the high one — and holding a key settles on an answer instead of
        // oscillating between them, which is what a toggle did: where the value
        // ended up was the parity of how many repeats the terminal sent.
        // `Signal::Flip` is the key that means "the other one", and does.
        Shape::YesOrNo => Some((by > 0).to_string()),
        Shape::OneOf(readings) => {
            let at = readings.iter().position(|reading| reading == from)?;
            let next = (at as i64 + by as i64).rem_euclid(readings.len() as i64) as usize;
            Some(readings[next].clone())
        }
        Shape::Range { least, most, step } => {
            let held: i64 = from.parse().ok()?;
            let moved = held + i64::from(by) * i64::from(*step);
            Some(moved.clamp(i64::from(*least), i64::from(*most)).to_string())
        }
    }
}

/// Why a Switch or an edit was not taken while a Refresh is out.
const WAITING_ON_A_REFRESH: &str = "The Refresh you asked for is still out, and it holds Perch's \
                                    own lock while it writes what it read. Press the key again \
                                    once it is back.";

/// Why the clear key did nothing at Global.
const NOTHING_ABOVE_GLOBAL: &str = "Global is the value that applies where nothing narrower is \
                                    said, so it has nothing above it to Inherit from and nothing \
                                    to clear back to.\n\
                                    Clearing works on a Group's page and on the Ungrouped page, \
                                    where it drops that Scope's Override.";

const ONLY_A_SETTING_IS_INHERITED: &str = "That is a fact about the Account rather than a Setting, \
                                           so there is no Override on it to clear.";

/// Why the clear key does nothing on the one key with no per-Scope form. It is
/// a Setting, so the sentence above would be wrong about it — and it is shown
/// on this page because this is where it takes effect rather than where it
/// lives.
const GLOBALS_ALONE: &str = "`cycle-ungrouped` is Global's alone: the Accounts it governs have no \
                             Group to carry a Setting, so there is no Override of it anywhere to \
                             clear (ADR 0017).";

const A_NAME_IS_TYPED: &str = "A name has no natural order to step through. Press `n` to type one.";

const A_NUMBER_IS_STEPPED: &str = "That one is a number rather than a yes or a no. The arrow keys step it, and Home and End \
     take it to the ends of its range.";

const A_FACT_RATHER_THAN_A_SETTING: &str = "That is something Anthropic says about the Account rather than something Perch was told, so \
     there is nothing here to change.";

const NOTHING_HERE_IS_NAMED: &str = "Nothing on this row is named. `n` names a Group on the last row of the sidebar, renames one \
     on its own row, and names the selected Account on its `alias` row.";

const NEITHER_IS_A_GROUP: &str = "Global is what applies where nothing narrower is said, and being in no Group is the absence of \
     a declaration rather than one somebody made (ADR 0017) — so neither is a Group somebody named, \
     and there is no name here to change. `n` renames a Group on its own row of the sidebar.";

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
fn listed<'a>(
    registry: &'a Registry,
    scope: &cycle::Scope,
    now: DateTime<Utc>,
) -> Vec<&'a Account> {
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
fn scopes(registry: &Registry) -> Vec<cycle::Scope> {
    let mut every: Vec<cycle::Scope> = registry
        .group_names()
        .map(|name| cycle::Scope::Group(name.to_string()))
        .collect();
    every.push(cycle::Scope::Ungrouped);

    let Some(active) = registry.active_account() else {
        return every;
    };
    let here = match &active.group {
        Some(name) => cycle::Scope::Group(name.clone()),
        None => cycle::Scope::Ungrouped,
    };
    let mut ordered = vec![here.clone()];
    ordered.extend(every.into_iter().filter(|scope| *scope != here));
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::Identity;
    use crate::registry::{CachedUtilization, Quarantine, Registry, Strategy, WindowUtilization};
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

    /// The Accounts the Status tab lists, in the order it lists them.
    fn listed(model: &Model) -> Vec<&str> {
        model.accounts().into_iter().map(Account::email).collect()
    }

    /// Moves the Status tab onto its Accounts row and into the table, which is
    /// where the two acting keys mean anything.
    ///
    /// By pressing the keys rather than by assigning the cursor, as every
    /// placement helper here does. A test that put the cursor somewhere no
    /// keystroke reaches is a test asserting about a state nobody can be in —
    /// and `moved`, which is the whole of how a cursor gets anywhere, was
    /// exercised by no unit test at all.
    fn on_the_accounts(model: &mut Model) {
        while model.status() != StatusRow::Accounts {
            let _ = model.act_on(Signal::Down);
        }
        let _ = model.act_on(Signal::Right);
        assert_eq!(model.column, Column::Content, "inside the table");
    }

    #[test]
    fn the_views_are_a_ring_in_both_directions() {
        let mut model = model_of(&[]);
        assert_eq!(model.tab, Tab::Status);

        assert_eq!(model.act_on(Signal::NextTab), Asked::Nothing);
        assert_eq!(model.tab, Tab::Config);
        assert_eq!(model.act_on(Signal::NextTab), Asked::Nothing);
        assert_eq!(model.tab, Tab::Status);

        assert_eq!(model.act_on(Signal::PreviousTab), Asked::Nothing);
        assert_eq!(model.tab, Tab::Config);
    }

    /// The sidebar is walked with the arrow keys, and stays walkable: `→`
    /// steps into the page and `←` comes back, so the table of Accounts does
    /// not swallow the key that reaches the other rows.
    #[test]
    fn the_status_sidebar_is_walked_with_the_arrow_keys_and_stays_reachable() {
        let mut model = model_of(&["one@example.com", "two@example.com"]);
        assert_eq!(model.status(), StatusRow::Overview);

        let _ = model.act_on(Signal::Down);
        assert_eq!(model.status(), StatusRow::Accounts);
        let _ = model.act_on(Signal::Down);
        assert_eq!(model.status(), StatusRow::Config);

        let _ = model.act_on(Signal::Up);
        let _ = model.act_on(Signal::Right);
        let _ = model.act_on(Signal::Down);
        assert_eq!(model.cursor, 1, "inside, the same key moves the table");
        assert_eq!(model.status(), StatusRow::Accounts);

        let _ = model.act_on(Signal::Left);
        let _ = model.act_on(Signal::Down);
        assert_eq!(
            model.status(),
            StatusRow::Config,
            "and `←` gives the key back to the sidebar"
        );
    }

    #[test]
    fn the_cursor_stops_at_both_ends_of_the_listing() {
        let mut model = model_of(&["one@example.com", "two@example.com"]);
        on_the_accounts(&mut model);

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
        on_the_accounts(&mut model);
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
            listed(&model),
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
            listed(&model),
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
        on_the_accounts(&mut model);
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
    #[test]
    fn a_report_does_not_outlive_the_account_it_was_about() {
        let mut model = model_holding(vec![
            in_group(used(account("first@example.com"), 10.0), "work"),
            in_group(used(account("second@example.com"), 50.0), "work"),
        ]);
        on_the_accounts(&mut model);
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
        on_the_accounts(&mut model);
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
            on_the_accounts(&mut model);

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
        on_the_accounts(&mut model);
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

    /// And an edit is refused in the same words, for the same reason and in the
    /// same place — one rule explains both.
    #[test]
    fn an_edit_waits_for_the_refresh_in_the_words_a_switch_waits_in() {
        let mut model = model_of(&["one@example.com"]);
        let _ = model.act_on(Signal::Refresh);
        on_the_config_setting(&mut model, Setting::WatcherMayAct);

        assert_eq!(model.act_on(Signal::Flip), Asked::Nothing);

        assert_eq!(model.said, vec![WAITING_ON_A_REFRESH.to_string()]);
    }

    /// And a deferred one is held rather than taken, for the same reason and
    /// about the same lock — then landed once the Refresh is back, because
    /// dropping it would be the save button this exists not to be.
    #[test]
    fn a_deferred_write_does_not_go_out_under_a_refresh_and_is_not_dropped_either() {
        let mut model = model_of(&["one@example.com"]);
        on_the_config_setting(&mut model, Setting::WatcherMarginPercent);
        let _ = model.act_on(Signal::Right);
        assert_eq!(model.act_on(Signal::Refresh), Asked::ForARefresh);

        for _ in 0..SETTLES_AFTER + 2 {
            assert_eq!(model.settled(), None, "nothing while the Refresh is out");
        }

        model.refreshed(Refreshed::nothing_read(vec![]));
        let settled = std::iter::repeat_with(|| model.settled())
            .take(SETTLES_AFTER + 1)
            .flatten()
            .next();
        assert_eq!(
            settled,
            Some(Edit::Setting {
                scope: Scope::Global,
                key: Setting::WatcherMarginPercent,
                value: Some("15".to_string()),
            }),
            "and it lands the moment the lock is free again",
        );
    }

    /// Two steps on two rows inside the debounce are two changes somebody made.
    /// Displacing the first with the second writes it rather than dropping it.
    #[test]
    fn stepping_another_row_before_the_first_settles_writes_the_first() {
        let mut model = model_of(&["one@example.com"]);
        on_the_config_setting(&mut model, Setting::WatcherThresholdPercent);
        let _ = model.act_on(Signal::Right);

        on_the_config_setting(&mut model, Setting::WatcherMarginPercent);

        assert_eq!(
            model.act_on(Signal::Right),
            Asked::ToWrite(Edit::Setting {
                scope: Scope::Global,
                key: Setting::WatcherThresholdPercent,
                value: Some("85".to_string()),
            }),
            "the one being displaced goes out now"
        );
        assert_eq!(
            model.take_pending(),
            Some(Edit::Setting {
                scope: Scope::Global,
                key: Setting::WatcherMarginPercent,
                value: Some("15".to_string()),
            }),
            "and the one that displaced it is still waiting"
        );
    }

    /// Stepping the *same* row again is a correction rather than a second
    /// change, so it replaces what was waiting instead of writing it.
    #[test]
    fn stepping_the_same_row_again_replaces_what_was_waiting() {
        let mut model = model_of(&["one@example.com"]);
        on_the_config_setting(&mut model, Setting::WatcherThresholdPercent);

        assert_eq!(model.act_on(Signal::Right), Asked::Nothing);
        assert_eq!(model.act_on(Signal::Right), Asked::Nothing);

        assert_eq!(
            model.take_pending(),
            Some(Edit::Setting {
                scope: Scope::Global,
                key: Setting::WatcherThresholdPercent,
                value: Some("90".to_string()),
            }),
            "one write, of where the stepping got to"
        );
    }

    /// `cycle-ungrouped` is a Setting rather than a fact about an Account, so
    /// the clear key says the right thing about it: it is Global's alone and
    /// there is no Override of it anywhere to clear.
    #[test]
    fn the_clear_key_on_the_one_global_only_key_says_what_it_is() {
        let mut model = model_of(&["one@example.com"]);
        model.tab = Tab::Config;
        model.column = Column::Content;
        model.scope_row = 1;
        model.content_row = model
            .content_rows()
            .iter()
            .position(|row| *row == Row::CycleUngrouped)
            .expect("it is on the Ungrouped page");

        assert_eq!(model.act_on(Signal::Clear), Asked::Nothing);

        let said = model.said.join(" ");
        assert!(said.contains("Global's alone"), "{said}");
        assert!(
            !said.contains("fact about the Account"),
            "it is a Setting, and the sentence for a fact would be wrong: {said}"
        );
    }

    /// A Run lasts as long as somebody's session, so it is not something the
    /// loop takes and comes back from: the view ends with the Account to launch
    /// as, and the terminal is given back before anything is launched into it.
    #[test]
    fn the_run_key_ends_the_view_naming_the_account_to_launch() {
        let mut model = model_of(&["one@example.com", "two@example.com"]);
        on_the_accounts(&mut model);
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
        on_the_accounts(&mut model);
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
        on_the_accounts(&mut model);

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
            listed(&model),
            ["tired@example.com", "fresh@example.com"],
            "as held, which is how `perch list` shows them",
        );

        let mut saying_they_are = registry_of(held);
        saying_they_are.global.cycle_ungrouped = true;
        let model = Model::new(saying_they_are, at(12));
        assert_eq!(
            listed(&model),
            ["fresh@example.com", "tired@example.com"],
            "and ranked once something says a Cycle may move between them",
        );
    }

    // ---- The Config tab ----

    /// Puts the cursor on one Setting of Global's page.
    fn on_the_config_setting(model: &mut Model, setting: Setting) {
        on_the_row(model, &Scope::Global, Row::Setting(setting));
    }

    /// Puts the cursor on one row of a Scope's page, whichever kind of row it
    /// is.
    ///
    /// The Scope is found by name rather than counted to: the sidebar orders
    /// Groups alphabetically after Global and Ungrouped, so an index is a fact
    /// about the fixture rather than about the test.
    fn on_the_row(model: &mut Model, scope: &Scope, row: Row) {
        // Round the tabs until the Config page is showing with the keys back on
        // its sidebar. `Tab` rather than `←`, because `←` on a row that steps
        // is a *decrement* rather than a way out — `leftwards` says so, and it
        // means the sidebar cannot be reached by arrow key from a Setting at
        // all. Changing view resets the column, which is the way back.
        for _ in 0..Tab::ALL.len() {
            let _ = model.act_on(Signal::NextTab);
            if model.tab == Tab::Config && model.column == Column::Scopes {
                break;
            }
        }
        assert_eq!(model.tab, Tab::Config);
        assert_eq!(model.column, Column::Scopes, "the keys are on the sidebar");

        // Then up it and down to the Scope. Walked rather than assigned,
        // because stepping the sidebar is what resets the two cursors below it
        // — the behaviour a test that jumped straight to the row would be
        // assuming rather than exercising. Up first because the sidebar does
        // not wrap, so a cursor already past the Scope cannot walk down to it.
        let scopes = model.scope_rows().len();
        for _ in 0..scopes {
            let _ = model.act_on(Signal::Up);
        }
        for _ in 0..scopes {
            if model.scope_at_the_cursor() == ScopeRow::Scope(scope.clone()) {
                break;
            }
            let _ = model.act_on(Signal::Down);
        }
        assert_eq!(
            model.scope_at_the_cursor(),
            ScopeRow::Scope(scope.clone()),
            "the Scope is in the sidebar"
        );

        // Rightwards until the keys are acting on the content pane, which is
        // one column on a Scope holding no Accounts and two on one that does.
        while model.column != Column::Content {
            let _ = model.act_on(Signal::Right);
        }

        let rows = model.content_rows().len();
        for _ in 0..rows {
            if model.content_rows()[model.content_row] == row {
                break;
            }
            let _ = model.act_on(Signal::Down);
        }
        assert_eq!(
            model.content_rows()[model.content_row],
            row,
            "the row is on the page"
        );
    }

    /// Every row that can be stepped shows the write the arrow keys have made,
    /// while it is still waiting to land.
    ///
    /// This is what [`Model::defer`] promises — "the row shows what has been
    /// done to it rather than what is still on disk" — and it was true of the
    /// six Setting rows and false of the three below them. `value_of` overlaid
    /// the pending write and answers about a *Setting*, so the view had no way
    /// to ask the same question about a row that is not one and read the
    /// registry instead: hold `→` on the `group` row across four Groups and the
    /// value stepped underneath while the frame showed the Group it started on.
    #[test]
    fn a_stepped_row_shows_the_write_that_has_not_landed_yet() {
        // Global, Ungrouped, then `personal` and `work` — the sidebar order the
        // scope_row indices below are counting.
        let mut registry = registry_of(vec![
            in_group(account("one@example.com"), "work"),
            account("two@example.com"),
        ]);
        // A second Group to step into, so the `group` row has somewhere to walk.
        registry
            .declare_group("personal")
            .expect("the name is free");
        let mut model = Model::new(registry, at(12));
        let work = Scope::Group("work".to_string());
        let ungrouped = Scope::Ungrouped;

        // A Setting, which already worked — here so the row that behaved and
        // the rows that did not are pinned by one test.
        on_the_row(
            &mut model,
            &Scope::Global,
            Row::Setting(Setting::WatcherThresholdPercent),
        );
        let before = model.shown(
            &Scope::Global,
            &Row::Setting(Setting::WatcherThresholdPercent),
        );
        let _ = model.act_on(Signal::Right);
        assert_ne!(
            model
                .shown(
                    &Scope::Global,
                    &Row::Setting(Setting::WatcherThresholdPercent)
                )
                .0,
            before.0,
            "a Setting moves as it is stepped"
        );

        // `cycling`, on the Group page holding one@example.com.
        on_the_row(&mut model, &work, Row::Cycling);
        assert_eq!(model.shown(&work, &Row::Cycling).0, "true");
        let _ = model.act_on(Signal::Left);
        assert_eq!(
            model.shown(&work, &Row::Cycling).0,
            "false",
            "the row reads as what the key did, not as what is on disk"
        );

        // `group`, stepped twice, which is the case that could not be walked at
        // all: each press reads where the last one put it, so without the
        // overlay every press in a run lands on the same neighbour. Leftwards,
        // because the places are `(none)`, `personal`, `work` in that order and
        // the Account starts on the last of them.
        on_the_row(&mut model, &work, Row::Group);
        assert_eq!(model.shown(&work, &Row::Group).0, "work");
        let _ = model.act_on(Signal::Left);
        assert_eq!(
            model.shown(&work, &Row::Group).0,
            "personal",
            "one press moves it to the neighbour"
        );
        let _ = model.act_on(Signal::Left);
        assert_eq!(
            model.shown(&work, &Row::Group).0,
            registry::NO_GROUP,
            "and the next press moves it again rather than back to the same one"
        );

        // `cycle-ungrouped`, which is Global's and shown on the Ungrouped page.
        on_the_row(&mut model, &ungrouped, Row::CycleUngrouped);
        assert_eq!(model.shown(&ungrouped, &Row::CycleUngrouped).0, "false");
        let _ = model.act_on(Signal::Right);
        assert_eq!(
            model.shown(&ungrouped, &Row::CycleUngrouped).0,
            "true",
            "Global's own row moves too"
        );
    }

    /// A per-Account row asked of a page with no Account on it.
    ///
    /// `content_rows` only offers those five rows where there is an Account for
    /// them to be about, so the panel never asks — but `shown` is public and
    /// answers rather than panicking, because a row and the Account it is about
    /// are two pieces of state and a view that got them out of step should draw
    /// a blank rather than take the terminal down.
    #[test]
    fn a_per_account_row_on_a_page_with_no_account_reads_as_nothing() {
        let model = model_holding(vec![account("one@example.com")]);

        assert!(
            !model.content_rows().contains(&Row::Alias),
            "Global lists no Accounts of its own, so the row is not offered"
        );
        assert_eq!(
            model.shown(&Scope::Global, &Row::Alias),
            (String::new(), false)
        );
    }

    /// The dimming is a fact about where the value came from, and a write that
    /// has not landed does not change where it will come from.
    #[test]
    fn a_pending_write_moves_the_value_and_not_the_dimming() {
        let mut model = model_holding(vec![account("one@example.com")]);
        let ungrouped = Scope::Ungrouped;

        on_the_row(&mut model, &ungrouped, Row::CycleUngrouped);
        assert!(
            model.shown(&ungrouped, &Row::CycleUngrouped).1,
            "read from Global, so dimmed on the Ungrouped page"
        );

        let _ = model.act_on(Signal::Right);
        assert_eq!(model.shown(&ungrouped, &Row::CycleUngrouped).0, "true");
        assert!(
            model.shown(&ungrouped, &Row::CycleUngrouped).1,
            "and still Global's, so still dimmed"
        );
    }

    #[test]
    fn the_config_sidebar_is_global_then_ungrouped_then_every_group() {
        let model = model_holding(vec![
            in_group(account("one@example.com"), "work"),
            account("two@example.com"),
        ]);

        assert_eq!(
            model.scope_rows(),
            vec![
                ScopeRow::Scope(Scope::Global),
                ScopeRow::Scope(Scope::Ungrouped),
                ScopeRow::Scope(Scope::Group("work".to_string())),
                ScopeRow::NewGroup,
            ]
        );
    }

    /// Global has no middle column: it governs every Account there is, so a
    /// column of "the Accounts in Global" would be the whole registry under a
    /// Scope nobody is in.
    #[test]
    fn only_the_scopes_accounts_are_in_that_scopes_column() {
        let mut model = model_holding(vec![
            in_group(account("one@example.com"), "work"),
            account("two@example.com"),
        ]);
        model.tab = Tab::Config;

        assert!(model.scope_accounts().is_empty(), "Global");

        model.scope_row = 1;
        assert_eq!(
            model
                .scope_accounts()
                .iter()
                .map(|a| a.email())
                .collect::<Vec<_>>(),
            ["two@example.com"],
        );

        model.scope_row = 2;
        assert_eq!(
            model
                .scope_accounts()
                .iter()
                .map(|a| a.email())
                .collect::<Vec<_>>(),
            ["one@example.com"],
        );
    }

    /// A bool flips on one key, and the write is the command's own.
    #[test]
    fn one_key_flips_the_simplest_setting_there_is() {
        let mut model = model_of(&["one@example.com"]);
        on_the_config_setting(&mut model, Setting::WatcherMayAct);

        assert_eq!(
            model.act_on(Signal::Flip),
            Asked::ToWrite(Edit::Setting {
                scope: Scope::Global,
                key: Setting::WatcherMayAct,
                value: Some("true".to_string()),
            })
        );
    }

    /// A percentage steps in fives, and the ends of the range are one keystroke
    /// away.
    #[test]
    fn a_percentage_steps_in_fives_and_the_ends_are_one_keystroke_away() {
        let mut model = model_of(&["one@example.com"]);
        on_the_config_setting(&mut model, Setting::WatcherThresholdPercent);

        let _ = model.act_on(Signal::Right);
        assert_eq!(
            model
                .value_of(&Scope::Global, Setting::WatcherThresholdPercent)
                .0,
            "85"
        );
        let _ = model.act_on(Signal::Left);
        let _ = model.act_on(Signal::Left);
        assert_eq!(
            model
                .value_of(&Scope::Global, Setting::WatcherThresholdPercent)
                .0,
            "75"
        );

        let _ = model.act_on(Signal::Most);
        assert_eq!(
            model
                .value_of(&Scope::Global, Setting::WatcherThresholdPercent)
                .0,
            "100"
        );
        let _ = model.act_on(Signal::Least);
        assert_eq!(
            model
                .value_of(&Scope::Global, Setting::WatcherThresholdPercent)
                .0,
            "0"
        );
    }

    /// Holding an arrow down is one write when it stops rather than one per
    /// step — counted in frames, so the fake screen's "a wait nobody pressed
    /// anything in" is what drives it.
    #[test]
    fn a_run_of_steps_is_one_write_when_the_keys_stop() {
        let mut model = model_of(&["one@example.com"]);
        on_the_config_setting(&mut model, Setting::WatcherThresholdPercent);

        for _ in 0..4 {
            assert_eq!(model.act_on(Signal::Right), Asked::Nothing);
            assert_eq!(model.settled(), None, "nothing while the keys are moving");
        }

        // The frames nobody pressed anything in.
        assert_eq!(model.settled(), None);
        assert_eq!(
            model.settled(),
            Some(Edit::Setting {
                scope: Scope::Global,
                key: Setting::WatcherThresholdPercent,
                value: Some("100".to_string()),
            }),
            "one write, of where the stepping got to"
        );
        assert_eq!(model.settled(), None, "and not a second one");
    }

    /// Walking away mid-adjustment loses nothing: the way out flushes whatever
    /// has not been written.
    #[test]
    fn leaving_writes_what_the_arrow_keys_had_not_got_round_to() {
        let mut model = model_of(&["one@example.com"]);
        on_the_config_setting(&mut model, Setting::WatcherMarginPercent);
        let _ = model.act_on(Signal::Right);

        let _ = model.act_on(Signal::Leave);

        assert_eq!(
            model.take_pending(),
            Some(Edit::Setting {
                scope: Scope::Global,
                key: Setting::WatcherMarginPercent,
                value: Some("15".to_string()),
            })
        );
    }

    /// A Strategy steps between the readings there are, and cannot be stepped
    /// to one that does not exist.
    #[test]
    fn a_strategy_steps_between_the_readings_perch_implements() {
        let mut model = model_of(&["one@example.com"]);
        on_the_config_setting(&mut model, Setting::Strategy);

        let _ = model.act_on(Signal::Right);

        assert_eq!(
            model.value_of(&Scope::Global, Setting::Strategy).0,
            Strategy::SoonestReset.as_str()
        );
    }

    /// Clearing an Override is one key, and at Global it does nothing and says
    /// why rather than leaving somebody wondering whether it did.
    #[test]
    fn the_clear_key_drops_an_override_and_says_why_it_cannot_at_global() {
        let mut registry = registry_of(vec![in_group(account("one@example.com"), "work")]);
        registry
            .groups
            .get_mut("work")
            .unwrap()
            .watcher_threshold_percent = Some(55);
        let mut model = Model::new(registry, at(12));
        model.tab = Tab::Config;
        model.column = Column::Content;
        model.scope_row = 2;
        model.content_row = model
            .content_rows()
            .iter()
            .position(|row| *row == Row::Setting(Setting::WatcherThresholdPercent))
            .expect("it is on the page");

        assert_eq!(
            model.act_on(Signal::Clear),
            Asked::ToWrite(Edit::Setting {
                scope: Scope::Group("work".to_string()),
                key: Setting::WatcherThresholdPercent,
                value: None,
            })
        );

        on_the_config_setting(&mut model, Setting::WatcherThresholdPercent);
        assert_eq!(model.act_on(Signal::Clear), Asked::Nothing);
        assert!(
            model.said.join(" ").contains("nothing to clear back to"),
            "{:?}",
            model.said
        );
    }

    /// Where a value came from is shown, and an Inherited one shows Global's
    /// value rather than a blank.
    #[test]
    fn an_inherited_setting_shows_globals_value_and_says_where_it_came_from() {
        let mut registry = registry_of(vec![in_group(account("one@example.com"), "work")]);
        registry.global.settings.watcher_threshold_percent = 55;
        let model = Model::new(registry, at(12));
        let work = Scope::Group("work".to_string());

        assert_eq!(
            model.value_of(&work, Setting::WatcherThresholdPercent),
            ("55".to_string(), Scope::Global),
        );
    }

    /// A name is the one value with no natural step, so it is the one place the
    /// panel accepts typed input — and a collision is refused as it is
    /// confirmed, before anything is written.
    #[test]
    fn a_group_name_that_collides_is_refused_at_the_prompt() {
        let mut model = model_holding(vec![in_group(account("one@example.com"), "work")]);
        model.tab = Tab::Config;
        model.scope_row = model.scope_rows().len() - 1;

        let _ = model.act_on(Signal::Name);
        for letter in "work".chars() {
            let _ = model.act_on(Signal::Typed(letter));
        }
        assert_eq!(model.act_on(Signal::Switch), Asked::Nothing);

        assert!(model.prompt.is_some(), "the field stays open");
        assert!(
            model.said.join(" ").contains("already a Group"),
            "{:?}",
            model.said
        );
    }

    #[test]
    fn a_name_that_is_free_declares_the_group() {
        let mut model = model_of(&["one@example.com"]);
        model.tab = Tab::Config;
        model.scope_row = model.scope_rows().len() - 1;

        let _ = model.act_on(Signal::Name);
        for letter in "spare".chars() {
            let _ = model.act_on(Signal::Typed(letter));
        }

        assert_eq!(
            model.act_on(Signal::Switch),
            Asked::ToWrite(Edit::DeclareGroup("spare".to_string()))
        );
        assert!(model.prompt.is_none());
    }

    /// The Group being renamed can go while the field is open: a Refresh
    /// landing replaces the registry, and another `perch` may have forgotten
    /// that Group by then. The panel hands the write over anyway, because
    /// `perch group rename` is what refuses a Group nothing holds — in the
    /// sentence every mistyped Group name gets, which is not a sentence the
    /// screen owns.
    #[test]
    fn a_rename_of_a_group_that_has_since_gone_is_the_commands_to_refuse() {
        let mut model = model_holding(vec![in_group(account("one@example.com"), "work")]);
        model.tab = Tab::Config;
        model.scope_row = 2;
        let Some(ScopeRow::Scope(Scope::Group(_))) = Some(model.scope_at_the_cursor()) else {
            panic!(
                "the third sidebar row is the Group: {:?}",
                model.scope_at_the_cursor()
            )
        };
        let _ = model.act_on(Signal::Name);

        // What another `perch group remove` leaves behind, arriving the way a
        // landed Refresh arrives.
        model.now_holds(Registry::default());

        for _ in 0.."work".len() {
            let _ = model.act_on(Signal::Backspace);
        }
        for letter in "day-job".chars() {
            let _ = model.act_on(Signal::Typed(letter));
        }

        assert_eq!(
            model.act_on(Signal::Switch),
            Asked::ToWrite(Edit::RenameGroup {
                from: "work".to_string(),
                to: "day-job".to_string(),
            }),
            "the write is handed over rather than the panel deciding for itself"
        );
    }

    /// Every letter belongs to the name while a field is open, including the
    /// one that would otherwise leave.
    #[test]
    fn a_letter_typed_into_a_name_is_not_a_command() {
        let mut model = model_of(&["one@example.com"]);
        model.tab = Tab::Config;
        model.scope_row = model.scope_rows().len() - 1;
        let _ = model.act_on(Signal::Name);

        let _ = model.act_on(Signal::Typed('q'));
        let _ = model.act_on(Signal::Typed('r'));

        assert_eq!(model.leaving, None, "`q` was a letter");
        assert_eq!(model.refreshing, Refreshing::Unasked, "and so was `r`");
        assert_eq!(model.prompt.as_ref().map(|p| p.typed.as_str()), Some("qr"));
    }

    /// A refused write puts the row back to what was actually written: a value
    /// nobody wrote is not one anybody should be reading.
    #[test]
    fn a_refused_write_puts_the_row_back_to_what_is_on_disk() {
        let mut model = model_of(&["one@example.com"]);
        on_the_config_setting(&mut model, Setting::WatcherThresholdPercent);
        let _ = model.act_on(Signal::Right);
        assert_eq!(
            model
                .value_of(&Scope::Global, Setting::WatcherThresholdPercent)
                .0,
            "85"
        );

        // What the loop does with it: the frames nobody pressed anything in,
        // then the write, then whatever the command said.
        let edit = std::iter::repeat_with(|| model.settled())
            .take(SETTLES_AFTER + 1)
            .flatten()
            .next()
            .expect("it settles");
        assert_eq!(
            edit,
            Edit::Setting {
                scope: Scope::Global,
                key: Setting::WatcherThresholdPercent,
                value: Some("85".to_string()),
            }
        );
        model.wrote(vec!["another `perch` holds it".to_string()], None);

        assert_eq!(
            model
                .value_of(&Scope::Global, Setting::WatcherThresholdPercent)
                .0,
            "80",
            "what is on disk, rather than what was never written"
        );
        assert_eq!(model.said, vec!["another `perch` holds it".to_string()]);
    }
}
