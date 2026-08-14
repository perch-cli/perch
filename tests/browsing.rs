//! `perch tui` — the frame loop, driven with no terminal.
//!
//! The loop under test is the real one. What is faked is the terminal it draws
//! in ([`FakeScreen`], which keeps every frame as text) and the Refresh it can
//! ask for ([`FakeRefresher`], which answers when the test says so) — so these
//! say what somebody pressed and then read what they would have seen.

mod common;

use common::*;

use chrono::Duration;
use perch::commands::list;
use perch::host::fake::Effect;
use perch::host::{FakeHost, Host};
use perch::registry::Registry;
use perch::tui::fake::{FakeRefresher, FakeScreen};
use perch::tui::refresh::Refreshed;
use perch::tui::{Left, Signal};
use ratatui::style::Modifier;

/// Opens the TUI on this machine and does these things at it, with a Refresh
/// that never comes back — which is what one looks like for as long as it is
/// out, and what the ordinary case never asks for at all.
fn browse(host: &FakeHost, doing: Vec<Option<Signal>>) -> FakeScreen {
    let mut screen = FakeScreen::scripted(doing);
    browse_with(host, &mut screen, &mut FakeRefresher::out_for_ever());
    screen
}

/// The same, for the tests that are about what the Refresh does.
fn browse_with(host: &FakeHost, screen: &mut FakeScreen, refresher: &mut FakeRefresher) -> Left {
    let registry = perch::adopt::ensure_adopted(host).expect("the machine has a login to adopt");
    perch::tui::browse(host, registry, screen, refresher).expect("the TUI leaves cleanly")
}

/// The same again, for the tests about what the view was left for.
fn left_after(host: &FakeHost, doing: Vec<Option<Signal>>) -> Left {
    browse_with(
        host,
        &mut FakeScreen::scripted(doing),
        &mut FakeRefresher::out_for_ever(),
    )
}

/// The same again, holding exactly what the test says rather than what the
/// machine has.
fn browse_holding(host: &FakeHost, registry: Registry, doing: Vec<Option<Signal>>) -> FakeScreen {
    let mut screen = FakeScreen::scripted(doing);
    perch::tui::browse(
        host,
        registry,
        &mut screen,
        &mut FakeRefresher::out_for_ever(),
    )
    .expect("the TUI leaves cleanly");
    screen
}

/// A frame read as one run of words, for the assertions about a sentence rather
/// than about a line.
///
/// The Overview breaks what it says between words at whatever the terminal is
/// — a figure that lost `(as of 4m ago)` to the right-hand edge would be a
/// figure with no age on it (ADR 0015) — so where a whole sentence is what is
/// under test, the line breaks the terminal put in are not.
fn said(frame: &str) -> String {
    frame.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// The `Status` tab opens on the Overview, with the keys on the sidebar. One
/// `Down` reaches the `Accounts` row and one `Right` steps into the table,
/// which is where the two acting keys mean anything — so every test about
/// choosing an Account starts with both.
fn at_the_accounts(doing: Vec<Option<Signal>>) -> Vec<Option<Signal>> {
    [Some(Signal::Down), Some(Signal::Right)]
        .into_iter()
        .chain(doing)
        .collect()
}

/// And the `Config` row of the same sidebar, which is read rather than stepped
/// into.
fn at_what_governs_me(doing: Vec<Option<Signal>>) -> Vec<Option<Signal>> {
    [Some(Signal::Down), Some(Signal::Down)]
        .into_iter()
        .chain(doing)
        .collect()
}

/// And the `Config` tab is one `Tab` away, with the sidebar on Global.
fn at_the_config(doing: Vec<Option<Signal>>) -> Vec<Option<Signal>> {
    std::iter::once(Some(Signal::NextTab))
        .chain(doing)
        .collect()
}

/// A machine with two Accounts, both with figures four minutes old. Neither is
/// in a Group, which is the ordinary starting state (ADR 0017) — so the listing
/// is the order they were added rather than a ranking.
fn machine_with_figures() -> FakeHost {
    let host = machine_with_two_accounts();
    observed(&host, EMAIL, vec![window("5-hour", 42.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 7.0)]);
    host.forget_effects();
    host
}

/// The same, with both Accounts declared interchangeable — which is what makes
/// the listing a ranking, and the state every test about the order or about
/// acting on the top of it starts from. The active Account is the fuller of the
/// two, so the ranking puts the other first.
fn machine_with_a_group() -> FakeHost {
    let host = machine_with_figures();
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "work").0.expect("it moves");
    }
    host.forget_effects();
    host
}

/// ADR 0015: the picker draws what Perch already knows and asks Anthropic
/// nothing. A first frame that waited on the network is a picker that hangs
/// before showing anything.
#[test]
fn the_first_frame_is_drawn_from_cache_and_touches_no_network() {
    let host = machine_with_figures();

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    assert_eq!(
        screen.frames().len(),
        1,
        "one frame, then it was told to go"
    );
    assert!(
        screen.last_frame().contains(EMAIL),
        "{}",
        screen.last_frame()
    );
    assert!(
        host.http_calls().is_empty(),
        "drawing asked Anthropic {:?}",
        host.http_calls()
    );
}

#[test]
fn the_loop_ends_when_it_is_asked_to_leave() {
    let host = machine_with_figures();

    // The fake screen refuses to invent a keystroke, so a loop that ran on past
    // the one that ends it would fail here rather than hang.
    let screen = browse(&host, vec![None, None, Some(Signal::Leave)]);

    assert_eq!(screen.frames().len(), 3);
}

#[test]
fn both_views_are_named_on_screen_and_the_keys_are_said() {
    let host = machine_with_figures();

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(frame.contains("Status"), "{frame}");
    assert!(frame.contains("Config"), "{frame}");
    assert!(frame.contains("q quit"), "{frame}");
    assert!(frame.contains("Tab view"), "{frame}");
    assert!(frame.contains("r refresh"), "{frame}");
}

#[test]
fn the_accounts_are_listed_with_their_alias_group_and_state() {
    let host = machine_with_figures();
    declare_group(&host, "work");
    move_to_group(&host, EMAIL, "work").0.expect("it moves");
    set_alias(&host, "main", EMAIL).0.expect("it is named");
    disable_account(&host, SECOND_EMAIL)
        .0
        .expect("it is spared");

    let screen = browse(&host, at_the_accounts(vec![Some(Signal::Leave)]));

    let frame = screen.last_frame();
    assert!(frame.contains("Account"), "{frame}");
    assert!(frame.contains("main"), "{frame}");
    assert!(frame.contains("work"), "{frame}");
    assert!(frame.contains("disabled"), "{frame}");
    // The active Account is marked with a character rather than with a colour,
    // so an SSH session on a terminal without one still says where you are.
    assert!(
        frame
            .lines()
            .any(|line| line.contains(">* ") && line.contains(EMAIL)),
        "{frame}"
    );
}

/// ADR 0015: every figure is shown with its age, so a stale number is visibly
/// stale rather than quietly wrong.
#[test]
fn the_age_of_every_figure_is_on_the_overview() {
    let host = machine_with_figures();

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let frame = screen.last_frame();
    for said in [EMAIL, "5-hour", "42%", "(as of 4m ago)"] {
        assert!(frame.contains(said), "{said} is missing from\n{frame}");
    }
    assert_eq!(
        frame.matches("as of").count(),
        2,
        "the Headroom and each Quota Window carries its own age\n{frame}"
    );
}

/// The summary answers "where am I": the Account, the name it was given, and
/// what a bare Switch would Cycle within.
#[test]
fn the_overview_says_who_is_active_by_the_name_they_were_given_and_what_governs_them() {
    let host = machine_with_figures();
    set_alias(&host, "main", EMAIL).0.expect("it is named");
    declare_group(&host, "work");
    move_to_group(&host, EMAIL, "work").0.expect("it moves");

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(frame.contains("Alias"), "{frame}");
    assert!(frame.contains("main"), "{frame}");
    assert!(
        frame.contains("work — a bare Switch Cycles within it"),
        "{frame}"
    );
    assert!(frame.contains("Headroom"), "{frame}");
}

/// And where somebody is in no Group it says so plainly, with the reason
/// Cycling behaves differently for them (ADR 0017).
#[test]
fn the_overview_says_plainly_when_the_active_account_is_in_no_group() {
    let host = machine_with_figures();

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(frame.contains("in no Group"), "{frame}");
    assert!(frame.contains("cycle-ungrouped"), "{frame}");
}

/// A Quarantined Account's figures describe quota it cannot spend, so the
/// reason goes where the Headroom would be — and the repair goes with it,
/// because that is the next thing somebody reading it needs.
#[test]
fn a_quarantined_active_account_shows_why_and_what_ends_it() {
    let host = machine_with_figures();
    quarantine(&host, EMAIL);

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let frame = screen.last_frame();
    let said = frame.split_whitespace().collect::<Vec<&str>>().join(" ");
    assert!(said.contains("Quarantine"), "{frame}");
    assert!(said.contains("would not renew its Credential"), "{frame}");
    assert!(
        said.contains(&format!("`perch relogin {EMAIL}`")),
        "{frame}"
    );
}

/// A machine Perch has been left on nobody gets one line saying so and pointing
/// at the page that has something to do about it, rather than an empty frame.
#[test]
fn a_machine_with_no_active_account_gets_one_line_and_a_way_out_of_it() {
    let host = machine_with_figures();
    let mut nobody = registry_of(&host);
    nobody.active = None;

    let screen = browse_holding(&host, nobody, vec![Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(frame.contains("no active Account"), "{frame}");
    assert!(frame.contains("Accounts page"), "{frame}");
}

/// An Account has several Quota Windows at once and is limited by whichever
/// fills first, so each gets a row — and the row says how long the fill lasts
/// as well as what it is: 90% that comes back in twenty minutes and 90% that
/// comes back in four hours are the same number and opposite advice.
#[test]
fn every_quota_window_gets_a_row_with_its_fill_and_when_it_resets() {
    let host = machine_with_figures();
    observed(
        &host,
        EMAIL,
        vec![
            resetting("5-hour", 42.0, host.now() + Duration::hours(3)),
            resetting("7-day", 88.0, host.now() + Duration::hours(50)),
        ],
    );

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let frame = screen.last_frame();
    let row = |window: &str| -> String {
        frame
            .lines()
            .find(|line| line.trim_start().starts_with(window))
            .unwrap_or_else(|| panic!("{window} has a row of its own in\n{frame}"))
            .to_string()
    };
    assert!(row("5-hour").contains("42%"), "{frame}");
    assert!(row("5-hour").contains("resets"), "{frame}");
    assert!(row("5-hour").contains("(in 3h)"), "{frame}");
    assert!(row("7-day").contains("88%"), "{frame}");
    assert!(row("7-day").contains("(in 2d)"), "{frame}");
    assert_eq!(
        said(frame).matches("as of 4m ago").count(),
        3,
        "the Headroom and both windows carry their own age\n{frame}"
    );
}

/// ADR 0012: an Account is only ever as free as its fullest window, so the one
/// figure it is compared on comes from that window — and names it, so the claim
/// can be checked against the rows underneath rather than taken on trust.
#[test]
fn the_overview_shows_the_headroom_the_most_constrained_window_leaves() {
    let host = machine_with_figures();
    observed(
        &host,
        EMAIL,
        vec![window("5-hour", 4.0), window("7-day", 95.0)],
    );

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let frame = screen.last_frame();
    let heading = frame
        .lines()
        .find(|line| line.contains("Headroom"))
        .unwrap_or_else(|| panic!("the Headroom is on the Overview in\n{frame}"));
    assert!(heading.contains("5%"), "{heading}");
    assert!(
        heading.contains("7-day is its fullest"),
        "the generous window never hides the exhausted one: {heading}"
    );
    assert!(heading.contains("as of 4m ago"), "{heading}");
}

/// A Group has no single figure and never will: its Accounts sit on different
/// plans and Perch only ever sees percentages, so what it has left is said as a
/// count and one Account's own figure.
#[test]
fn a_group_says_how_many_accounts_still_have_headroom_and_how_much_the_best_has() {
    let host = machine_with_a_group();

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(frame.contains("Group `work`"), "{frame}");
    assert!(
        said(frame)
            .contains("Reserve: 2 of 2 Accounts have Headroom, the best 93% left (as of 4m ago)"),
        "{frame}"
    );
}

/// The same Reserve on a terminal narrow enough to wrap it.
///
/// `wrapped` indents what runs over by four and the Overview budgeted for two,
/// so every line that wrapped was two cells wider than the area it was drawn in
/// and ratatui cut the tail off it. The tail of this one is its age: the line
/// read `Reserve: ... the best 93% left (as of ago)`, which is a figure with no
/// age on it — the one thing ADR 0015 will not have, and the thing the comment
/// over that arithmetic says it is there to prevent.
///
/// Asserted through `said`, which puts the line back together across the breaks
/// the terminal made: where the words went is the terminal's business, and
/// whether they are all still there is not.
#[test]
fn a_reserve_keeps_its_age_on_a_terminal_that_wraps_it() {
    let host = machine_with_a_group();

    let mut screen = FakeScreen::sized(50, 40, vec![Some(Signal::Leave)]);
    browse_with(&host, &mut screen, &mut FakeRefresher::out_for_ever());

    let frame = screen.last_frame();
    assert!(
        said(frame).contains("the best 93% left (as of 4m ago)"),
        "a figure that lost its age to the right-hand edge:\n{frame}"
    );
}

/// Summing or averaging percentages across Accounts produces a number that
/// looks quantitative, is not, and is exactly the kind of number people plan
/// around. So every figure a Group's rows quote is one an Account reported.
#[test]
fn no_figure_on_the_tab_is_one_no_account_reported() {
    let host = machine_with_a_group();

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    // The two Accounts are 42% and 7% full, so the only honest percentages on
    // screen are those, the Headroom each leaves, and nothing else.
    let frame = screen.last_frame();
    let reported = ["42%", "7%", "58%", "93%"];
    let shown: Vec<&str> = frame
        .split(|character: char| !character.is_ascii_digit() && character != '%')
        .filter(|word| word.ends_with('%') && word.len() > 1)
        .collect();
    assert!(!shown.is_empty(), "{frame}");
    for percentage in &shown {
        assert!(
            reported.contains(percentage),
            "{percentage} is a figure no Account reported — a pooled or averaged \
             one: {shown:?}\n{frame}"
        );
    }
    for pooled in ["Total", "total", "average", "Average", "combined"] {
        assert!(!frame.contains(pooled), "{pooled} appears in\n{frame}");
    }
}

/// Within one window kind the comparison at least means something, which is
/// what answers "this Group is fine on the weekly window and empty on the
/// five-hour one" — the case a single figure per Account hides.
#[test]
fn a_groups_per_window_rows_are_one_per_quota_window_kind() {
    let host = machine_with_a_group();
    observed(
        &host,
        EMAIL,
        vec![window("5-hour", 99.0), window("7-day", 30.0)],
    );
    observed(
        &host,
        SECOND_EMAIL,
        vec![window("5-hour", 96.0), window("7-day", 12.0)],
    );

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let said = said(screen.last_frame());
    assert!(
        said.contains("5-hour emptiest 96% used across 2 Accounts (as of 4m ago)"),
        "the Group is nearly out on its five-hour window\n{said}"
    );
    assert!(
        said.contains("7-day emptiest 12% used across 2 Accounts (as of 4m ago)"),
        "and fine on its seven-day one\n{said}"
    );
}

/// ADR 0017: being in no Group is the absence of a declaration that Accounts
/// are interchangeable. A Reserve over Accounts nobody has said are
/// interchangeable would be a figure about a set that is not one.
#[test]
fn the_accounts_in_no_group_get_no_reserve_until_cycling_may_choose_them() {
    let host = machine_with_figures();

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(frame.contains("in no Group"), "{frame}");
    assert!(!frame.contains("Reserve"), "{frame}");
    assert!(
        said(frame).contains("only moves between these when you say it may"),
        "and it says what would have to change\n{frame}"
    );

    config_set(&host, &["cycle-ungrouped", "true"])
        .0
        .expect("they are declared interchangeable");
    let screen = browse(&host, vec![Some(Signal::Leave)]);

    assert!(
        said(screen.last_frame())
            .contains("Reserve: 2 of 2 Accounts have Headroom, the best 93% left"),
        "{}",
        screen.last_frame()
    );
}

/// A Quarantined Credential does not work and a Disabled Account is never
/// chosen, so neither is part of what a Group has left to draw on — and a count
/// that quietly dropped them would not add up to the Accounts on screen.
#[test]
fn what_a_cycle_may_not_choose_is_not_counted_as_something_the_group_has() {
    let host = machine_with_a_group();
    disable_account(&host, SECOND_EMAIL)
        .0
        .expect("it is spared");

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(
        said(frame).contains("Reserve: 1 of 1 Account has Headroom, the best 58% left"),
        "{frame}"
    );
    assert!(
        said(frame).contains("1 disabled, so nothing Cycles to it."),
        "{frame}"
    );

    // And it is still listed, with its own figures, on the page that lists them.
    let screen = browse(&host, at_the_accounts(vec![Some(Signal::Leave)]));
    assert!(
        screen.last_frame().contains(SECOND_EMAIL),
        "{}",
        screen.last_frame()
    );
}

/// A Group's figures are read from cache like every other figure, so a Refresh
/// moves them and their age — and a failed one leaves them standing (ADR 0018).
/// Said of a Group rather than of one Account, because the Reserve and the
/// per-window rows are worked out afresh from whatever the registry now holds.
#[test]
fn a_refresh_moves_the_groups_figures_and_a_failed_one_leaves_them_standing() {
    let host = machine_with_a_group();
    let mut fresher = registry_of(&host);
    for (email, used) in [(EMAIL, 12.0), (SECOND_EMAIL, 3.0)] {
        fresher
            .account_mut(email)
            .expect("an Account Perch holds")
            .utilization = Some(perch::registry::CachedUtilization {
            observed_at: host.now(),
            windows: vec![window("5-hour", used)],
        });
    }
    let mut refresher = FakeRefresher::answering(
        Refreshed {
            registry: Some(fresher),
            notes: Vec::new(),
        },
        1,
    );
    let mut screen = FakeScreen::scripted(vec![Some(Signal::Refresh), None, Some(Signal::Leave)]);

    browse_with(&host, &mut screen, &mut refresher);

    assert!(
        said(&screen.frames()[0]).contains("the best 93% left (as of 4m ago)"),
        "the Reserve before the Refresh\n{}",
        screen.frames()[0]
    );
    let frame = screen.last_frame();
    assert!(
        said(frame).contains("the best 97% left (as of just now)"),
        "the Reserve moved with the figures it is made of\n{frame}"
    );
    assert!(
        said(frame).contains("5-hour emptiest 3% used across 2 Accounts (as of just now)"),
        "{frame}"
    );

    // And one that could not read anything leaves every figure with the age it
    // had, rather than the Group's rows emptying because Anthropic was busy.
    let mut failed = FakeRefresher::answering(
        Refreshed::nothing_read(vec!["overflow@example.com: no answer".to_string()]),
        0,
    );
    let mut screen = FakeScreen::scripted(vec![Some(Signal::Refresh), Some(Signal::Leave)]);

    browse_with(&host, &mut screen, &mut failed);

    let frame = screen.last_frame();
    assert!(frame.contains("no answer"), "{frame}");
    assert!(
        said(frame).contains("the best 93% left (as of 4m ago)"),
        "the Reserve is still there, with its age\n{frame}"
    );
}

/// A Group with nothing left says so rather than counting to zero, and each
/// Account says it is exhausted rather than showing a Headroom of nought.
#[test]
fn a_group_with_no_room_left_says_what_is_in_the_way_and_how_old_that_is() {
    let host = machine_with_a_group();
    observed(&host, EMAIL, vec![window("5-hour", 100.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 100.0)]);

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let said = said(screen.last_frame());
    assert!(
        said.contains("Reserve: none of 2 Accounts have Headroom (2 exhausted)"),
        "{said}"
    );
    assert!(
        said.contains("Read 4m ago at the oldest."),
        "a count read from cache says how old the readings behind it are\n{said}"
    );
    assert!(said.contains("Headroom exhausted (as of 4m ago)"), "{said}");
}

/// Fill and room are both percentages, and two of them an inch apart are told
/// apart by a word rather than by context.
#[test]
fn a_figure_says_whether_it_is_room_left_or_quota_used() {
    let host = machine_with_a_group();

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let said = said(screen.last_frame());
    assert!(said.contains("the best 93% left"), "{said}");
    assert!(said.contains("emptiest 7% used"), "{said}");
    assert!(
        said.contains("5-hour 42% used no reset time cached"),
        "and the active Account's own row says it too\n{said}"
    );
}

/// "No figure" and "plenty of room" are opposite pieces of advice.
#[test]
fn an_account_never_observed_says_so_rather_than_showing_zero() {
    let host = machine_with_two_accounts();

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    assert!(
        screen.last_frame().contains("never observed"),
        "{}",
        screen.last_frame()
    );
}

#[test]
fn the_views_are_reached_by_key_and_come_back() {
    let host = machine_with_figures();

    let screen = browse(
        &host,
        vec![
            Some(Signal::NextTab),
            Some(Signal::PreviousTab),
            Some(Signal::Leave),
        ],
    );

    let frames = screen.frames();
    assert!(
        frames[1].contains("Ungrouped"),
        "the second view is the Config panel\n{}",
        frames[1]
    );
    assert!(
        !frames[2].contains("Ungrouped"),
        "and back to the first\n{}",
        frames[2]
    );
}

/// A terminal that changed size is redrawn at the size it now is, rather than
/// leaving the parts of the old frame that nothing wrote over.
#[test]
fn a_resize_is_redrawn_at_the_new_size() {
    let host = machine_with_figures();
    let mut screen = FakeScreen::sized(
        80,
        24,
        vec![Some(Signal::Resized(48, 8)), Some(Signal::Leave)],
    );

    browse_with(&host, &mut screen, &mut FakeRefresher::out_for_ever());

    let frame = screen.last_frame();
    assert!(frame.lines().count() <= 8, "{frame}");
    assert!(
        frame.lines().all(|line| line.chars().count() <= 48),
        "{frame}"
    );
    assert!(
        frame.contains("Accounts"),
        "still drawn, not corrupted\n{frame}"
    );
}

/// A terminal too narrow for the tab bar and the active Account both keeps the
/// bar: which view this is, is the thing the keys act on.
#[test]
fn a_narrow_terminal_keeps_the_views_legible_and_drops_the_label() {
    let host = machine_with_figures();
    let mut screen = FakeScreen::sized(40, 10, vec![Some(Signal::Leave)]);

    browse_with(&host, &mut screen, &mut FakeRefresher::out_for_ever());

    let bar = screen
        .last_frame()
        .lines()
        .next()
        .expect("a frame")
        .to_string();
    assert!(bar.contains("Status"), "{bar}");
    assert!(bar.contains("Config"), "{bar}");
    assert!(!bar.contains("active:"), "there is no room for it: {bar}");
}

/// ADR 0018 wants what could not be read said by name, and the clause naming
/// the command that repairs it is the end of the sentence — so a note is broken
/// between words rather than cut at the width.
#[test]
fn a_note_too_long_for_the_terminal_is_broken_rather_than_cut() {
    let host = machine_with_figures();
    let repair = "spare@example.com: Anthropic would not renew its Credential. \
                  `perch relogin spare@example.com` repairs it.";
    let mut refresher =
        FakeRefresher::answering(Refreshed::nothing_read(vec![repair.to_string()]), 0);
    let mut screen = FakeScreen::scripted(vec![Some(Signal::Refresh), None, Some(Signal::Leave)]);

    browse_with(&host, &mut screen, &mut refresher);

    // The note reaches the screen whole, across however many lines it takes:
    // its last clause is the one naming the command that repairs the Account.
    let frame = screen.last_frame();
    let said: Vec<&str> = frame
        .lines()
        .filter(|line| line.contains("spare@example.com") || line.contains("repairs"))
        .flat_map(str::split_whitespace)
        .collect();

    assert_eq!(
        said,
        repair.split_whitespace().collect::<Vec<&str>>(),
        "{frame}"
    );
}

#[test]
fn nothing_is_asked_of_anthropic_until_somebody_asks_for_a_refresh() {
    let host = machine_with_figures();
    let mut refresher = FakeRefresher::out_for_ever();
    let mut screen = FakeScreen::scripted(vec![
        Some(Signal::Down),
        Some(Signal::NextTab),
        Some(Signal::Leave),
    ]);

    browse_with(&host, &mut screen, &mut refresher);

    assert!(refresher.asked().is_empty(), "{:?}", refresher.asked());
}

/// Every read spends from an hourly budget that does not refill early (ADR
/// 0015), so a Refresh covers exactly the Accounts on screen.
#[test]
fn a_refresh_reads_the_accounts_that_are_on_screen() {
    let host = machine_with_figures();
    let mut refresher = FakeRefresher::out_for_ever();
    let mut screen = FakeScreen::scripted(vec![Some(Signal::Refresh), Some(Signal::Leave)]);

    browse_with(&host, &mut screen, &mut refresher);

    // In the order they are shown, which for two ungrouped Accounts is the
    // order they were added.
    assert_eq!(
        refresher.asked(),
        [vec![EMAIL.to_string(), SECOND_EMAIL.to_string()]]
    );
}

/// The whole reason a Refresh is taken off the frame loop: the display goes on
/// answering while Anthropic is being waited on.
#[test]
fn the_display_keeps_answering_while_a_refresh_is_out() {
    let host = machine_with_figures();
    let mut refresher = FakeRefresher::out_for_ever();
    let mut screen = FakeScreen::scripted(vec![
        Some(Signal::Refresh),
        None,
        Some(Signal::NextTab),
        Some(Signal::Leave),
    ]);

    browse_with(&host, &mut screen, &mut refresher);

    assert_eq!(screen.frames().len(), 4, "it drew all the way through");
    assert!(screen.ever_said("Refreshing"), "{:?}", screen.frames());
    assert!(
        screen.last_frame().contains("Ungrouped"),
        "the key that changes view still worked\n{}",
        screen.last_frame()
    );
    assert_eq!(
        refresher.asked().len(),
        1,
        "and the second frame did not ask again"
    );
}

#[test]
fn a_refresh_that_lands_replaces_the_figures_and_their_age() {
    let host = machine_with_figures();
    let mut fresher = registry_of(&host);
    fresher
        .account_mut(EMAIL)
        .expect("an Account Perch holds")
        .utilization = Some(perch::registry::CachedUtilization {
        observed_at: host.now(),
        windows: vec![window("5-hour", 3.0)],
    });
    let mut refresher = FakeRefresher::answering(
        Refreshed {
            registry: Some(fresher),
            notes: Vec::new(),
        },
        1,
    );
    let mut screen = FakeScreen::scripted(vec![Some(Signal::Refresh), None, Some(Signal::Leave)]);

    browse_with(&host, &mut screen, &mut refresher);

    assert!(
        said(&screen.frames()[0]).contains("42% used no reset time cached (as of 4m ago)"),
        "the figure before the Refresh\n{}",
        screen.frames()[0]
    );
    let frame = screen.last_frame();
    assert!(frame.contains("3%"), "{frame}");
    assert!(frame.contains("(as of just now)"), "{frame}");
}

/// A Refresh that came back is not one that is still out, so `r` works again.
///
/// The loop guards the key on `outstanding()` — one Refresh at a time, because
/// each spends from an hourly budget that does not refill early (ADR 0015) —
/// and the whole of that guard is that the flag comes back down when the answer
/// lands. Nothing at this level said so, and a `FakeRefresher` that could only
/// answer once would have made the test that says it pass against a frozen loop
/// rather than fail.
#[test]
fn a_second_refresh_is_asked_for_once_the_first_has_landed() {
    let host = machine_with_figures();
    let mut refresher = FakeRefresher::answering(
        Refreshed {
            registry: Some(registry_of(&host)),
            notes: Vec::new(),
        },
        1,
    );
    let mut screen = FakeScreen::scripted(vec![
        Some(Signal::Refresh),
        // The frame the first one lands on.
        None,
        Some(Signal::Refresh),
        None,
        Some(Signal::Leave),
    ]);

    browse_with(&host, &mut screen, &mut refresher);

    assert_eq!(
        refresher.asked().len(),
        2,
        "the second `r` reached the Refresher rather than being swallowed by a \
         flag nothing put back down"
    );
    assert!(
        !screen.last_frame().contains("Refreshing"),
        "and the display is not still waiting on one that came back:\n{}",
        screen.last_frame()
    );
}

/// ADR 0018: a Refresh that failed degrades the display rather than emptying
/// it. The figures stand with the age they had, and what could not be read is
/// said beside them.
#[test]
fn a_refresh_that_failed_leaves_the_figures_standing_and_says_so() {
    let host = machine_with_figures();
    let mut refresher = FakeRefresher::answering(
        Refreshed::nothing_read(vec![
            "overflow@example.com: no answer from Anthropic".to_string(),
        ]),
        0,
    );
    let mut screen = FakeScreen::scripted(vec![Some(Signal::Refresh), Some(Signal::Leave)]);

    browse_with(&host, &mut screen, &mut refresher);

    let frame = screen.last_frame();
    assert!(frame.contains("no answer from Anthropic"), "{frame}");
    assert!(
        frame.contains("42%"),
        "the figures are still there\n{frame}"
    );
    assert!(frame.contains("(as of 4m ago)"), "with their age\n{frame}");
}

#[test]
fn a_perch_holding_nothing_says_so_rather_than_drawing_an_empty_table() {
    let host = machine_with_figures();

    let screen = browse_holding(&host, Registry::default(), vec![Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(frame.contains("No Accounts yet"), "{frame}");
    assert!(frame.contains("perch add"), "{frame}");
}

/// And `r` on that machine asks for nothing. A Refresh takes Perch's own lock
/// exclusively and has to be waited out on the way `q`, so one over no Accounts
/// is a screen saying "Refreshing", a thread that may sit on another `perch`'s
/// lock, and up to ten seconds of `q` doing nothing — to read nought Accounts.
#[test]
fn refreshing_a_perch_holding_nothing_asks_for_nothing() {
    let host = machine_with_figures();
    let mut refresher = FakeRefresher::out_for_ever();
    let mut screen = FakeScreen::scripted(vec![Some(Signal::Refresh), Some(Signal::Leave)]);

    perch::tui::browse(&host, Registry::default(), &mut screen, &mut refresher)
        .expect("the TUI leaves cleanly");

    assert!(
        refresher.asked().is_empty(),
        "no Refresh was asked for: {:?}",
        refresher.asked()
    );
    assert!(
        !refresher.was_waited_for(),
        "and `q` waited for nothing on the way out"
    );
}

/// A Refresh writes the registry under Perch's own lock, and leaving the TUI
/// is the end of the process — so an exit that walked away from one would leave
/// that lock behind for the next command to wait out.
#[test]
fn leaving_while_a_refresh_is_out_waits_for_it_and_says_why() {
    let host = machine_with_figures();
    let mut refresher = FakeRefresher::out_for_ever();
    let mut screen = FakeScreen::scripted(vec![Some(Signal::Refresh), Some(Signal::Leave)]);
    browse_with(&host, &mut screen, &mut refresher);

    let mut said = Vec::new();
    perch::tui::finish(&mut refresher, &mut said).expect("it says so and waits");

    let said = String::from_utf8(said).expect("output is UTF-8");
    assert!(said.contains("Finishing the Refresh"), "{said}");
    assert!(said.contains("lock"), "{said}");
    assert!(refresher.was_waited_for());
}

#[test]
fn leaving_with_nothing_out_waits_for_nothing_and_says_nothing() {
    let host = machine_with_figures();
    let mut refresher = FakeRefresher::out_for_ever();
    let mut screen = FakeScreen::scripted(vec![Some(Signal::Leave)]);
    browse_with(&host, &mut screen, &mut refresher);

    let mut said = Vec::new();
    perch::tui::finish(&mut refresher, &mut said).expect("there is nothing to wait for");

    assert!(said.is_empty(), "{said:?}");
    assert!(!refresher.was_waited_for());
}

/// The ranking `perch switch` makes is what the listing shows, with the figure
/// it was made on beside it — so the order can be checked rather than taken on
/// trust.
#[test]
fn the_accounts_are_listed_in_the_order_a_switch_would_rank_them() {
    let host = machine_with_a_group();

    let screen = browse(&host, at_the_accounts(vec![Some(Signal::Leave)]));

    let frame = screen.last_frame();
    assert!(frame.contains("Headroom"), "{frame}");
    // Every row of the listing, and not the tab bar's "active:" label.
    let listed: Vec<&str> = frame
        .lines()
        .filter(|line| line.contains('@') && !line.contains("active:"))
        .map(|line| line.trim())
        .collect();
    assert_eq!(listed.len(), 2, "{frame}");
    assert!(
        listed[0].contains(SECOND_EMAIL) && listed[0].ends_with("93%"),
        "the emptier Account is at the top with its Headroom: {listed:?}"
    );
    assert!(
        listed[1].contains(EMAIL) && listed[1].ends_with("58%"),
        "{listed:?}"
    );
}

/// Disabled is never a statement about whether the Account works and
/// Quarantined always is, so the two read differently — and neither is ever
/// mistaken for an Account that simply has no room.
#[test]
fn disabled_and_quarantined_are_distinguishable_at_a_glance() {
    let host = machine_with_three_accounts();
    observed(&host, THIRD_EMAIL, vec![window("5-hour", 1.0)]);
    disable_account(&host, SECOND_EMAIL)
        .0
        .expect("it is spared");
    quarantine(&host, THIRD_EMAIL);

    let screen = browse(&host, at_the_accounts(vec![Some(Signal::Leave)]));

    let frame = screen.last_frame();
    let row = |email: &str| -> String {
        frame
            .lines()
            .find(|line| line.contains(email))
            .unwrap_or_else(|| panic!("{email} is listed in\n{frame}"))
            .to_string()
    };
    assert!(row(SECOND_EMAIL).contains("disabled"), "{frame}");
    assert!(!row(SECOND_EMAIL).contains("quarantined"), "{frame}");
    assert!(row(THIRD_EMAIL).contains("quarantined"), "{frame}");
    assert!(
        frame
            .lines()
            .last()
            .is_some_and(|keys| keys.contains("run")),
        "{frame}"
    );
}

/// A Switch from the picker is the same Switch as everywhere: the outgoing
/// Credential Captured into its own Profile first, then the incoming one made
/// live, under Claude Code's locks (ADR 0006).
#[test]
fn enter_switches_to_the_account_under_the_cursor() {
    let host = machine_with_a_group();
    let was_live = credential_of(&host, EMAIL);

    // The cursor starts on the emptiest Account, which is the second one.
    let screen = browse(
        &host,
        at_the_accounts(vec![Some(Signal::Switch), None, Some(Signal::Leave)]),
    );

    assert_eq!(registry_of(&host).active.as_deref(), Some(SECOND_EMAIL));
    assert_eq!(
        credential_of(&host, EMAIL),
        was_live,
        "the outgoing Credential was Captured into its own Profile",
    );
    assert!(
        host.effects()
            .iter()
            .any(|effect| format!("{effect:?}").contains("oauth_refresh.lock")),
        "the Switch took Claude Code's locks: {:?}",
        host.effects()
    );
    let frame = screen.last_frame();
    assert!(frame.contains("Captured"), "{frame}");
    assert!(
        frame.contains(&format!("Switched to {SECOND_EMAIL}")),
        "{frame}"
    );
    assert!(
        frame
            .lines()
            .any(|line| line.contains(">* ") && line.contains(SECOND_EMAIL)),
        "the marker moved with it\n{frame}"
    );
}

/// What the Switch remarks on is shown in the frame rather than printed.
///
/// `Host::note` writes to stderr, which under the picker is the alternate
/// screen: the line lands in the middle of the display and stays there until
/// something redraws over it, and then it is gone for good. ADR 0016 settled
/// this for the Refresh thread, which runs against a Host of its own; the
/// Switch the picker performs runs against the process's, and was missed.
///
/// Invisible from a test until now, because a `FakeHost` never printed a remark
/// in the first place — it only ever kept them, which is the behaviour the real
/// one has had to be told to adopt.
#[test]
fn what_a_switch_remarked_on_is_shown_in_the_frame_rather_than_printed_over_it() {
    let host = machine_with_a_group();
    // A plaintext copy in the Profile the Capture writes into, which the write
    // supersedes and cannot remove: a remark on the way past rather than
    // anything that stops the Switch (ADR 0020).
    let superseded = store_of(&host, EMAIL).credentials_file;
    let host = host
        .with_file(&superseded, CREDENTIAL)
        .with_undeletable_file(&superseded, "Operation not permitted");

    let screen = browse(
        &host,
        at_the_accounts(vec![Some(Signal::Switch), None, Some(Signal::Leave)]),
    );

    let frame = screen.last_frame();
    assert!(
        frame.contains("superseded copy"),
        "the remark is on the screen the user is looking at:\n{frame}"
    );
    assert!(
        frame.contains(&format!("Switched to {SECOND_EMAIL}")),
        "beside what the command itself said:\n{frame}"
    );
}

/// A failed Refresh's notes stand until something else is done, and on a short
/// terminal there is only room for so many lines — so what a key just did is
/// said first and the older news is what gets counted rather than shown.
#[test]
fn what_a_switch_said_is_not_pushed_off_the_screen_by_an_older_refresh() {
    let host = machine_with_a_group();
    let unread: Vec<String> = (0..4)
        .map(|which| format!("account-{which}@example.com: no answer from Anthropic"))
        .collect();
    let mut refresher = FakeRefresher::answering(Refreshed::nothing_read(unread), 0);
    let mut screen = FakeScreen::sized(
        80,
        12,
        at_the_accounts(vec![
            Some(Signal::Refresh),
            Some(Signal::Switch),
            Some(Signal::Leave),
        ]),
    );

    browse_with(&host, &mut screen, &mut refresher);

    let frame = screen.last_frame();
    assert!(
        frame.contains(&format!("Switched to {SECOND_EMAIL}")),
        "{frame}"
    );
    assert!(
        frame.contains("more."),
        "the older news is counted\n{frame}"
    );
}

/// Rewriting Credentials to land where you already are is the one thing a
/// Switch can do that is worse than doing nothing, and the picker is where an
/// arrow key makes it easy to ask for.
#[test]
fn switching_to_the_account_that_is_already_active_says_there_is_nothing_to_do() {
    let host = machine_with_a_group();

    // Down onto the active Account, which the ranking put second.
    let screen = browse(
        &host,
        at_the_accounts(vec![
            Some(Signal::Down),
            Some(Signal::Switch),
            Some(Signal::Leave),
        ]),
    );

    let frame = screen.last_frame();
    assert!(frame.contains("already the active Account"), "{frame}");
    assert!(frame.contains("Nothing was changed"), "{frame}");
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
}

/// A Quarantine is never a statement that the Account is gone, so it stays
/// listed and stays selectable — and choosing it names the one command that
/// ends it rather than failing obscurely.
#[test]
fn choosing_a_quarantined_account_names_perch_relogin() {
    let host = machine_with_a_group();
    quarantine(&host, SECOND_EMAIL);

    // Down onto it: a Quarantined Account is one no Cycle would choose, so the
    // ranking puts it below every Account one would.
    for key in [Signal::Switch, Signal::Run] {
        let screen = browse(
            &host,
            at_the_accounts(vec![Some(Signal::Down), Some(key), Some(Signal::Leave)]),
        );

        let frame = screen.last_frame();
        // Read across the line breaks the terminal put in: the refusal is
        // broken between words to fit, and the clause naming the repair is the
        // end of a sentence rather than the end of a line.
        let said = frame.split_whitespace().collect::<Vec<&str>>().join(" ");
        assert!(said.contains("is Quarantined"), "{key:?}\n{frame}");
        assert!(
            said.contains(&format!("`perch relogin {SECOND_EMAIL}`")),
            "{key:?}\n{frame}"
        );
        assert_eq!(
            registry_of(&host).active.as_deref(),
            Some(EMAIL),
            "{key:?} changed nothing"
        );
    }
}

/// A Run lasts as long as somebody's session, so the view ends with the Account
/// to launch as rather than trying to come back from it: the terminal is given
/// back, and only then is anything launched into it.
#[test]
fn the_run_key_leaves_the_view_naming_the_account_to_launch() {
    let host = machine_with_a_group();

    assert_eq!(
        left_after(
            &host,
            at_the_accounts(vec![Some(Signal::Down), Some(Signal::Run)])
        ),
        Left::ToRun(EMAIL.to_string()),
    );
    assert_eq!(
        left_after(&host, vec![Some(Signal::Leave)]),
        Left::Alone,
        "`q` leaves and launches nothing",
    );
}

/// The two acting keys are confined to the same place (ADR 0011), and only one
/// of them was asking about the column. With the sidebar cursor on `Accounts`
/// but the keys still in the sidebar, `Enter` moved right and `x` handed the
/// terminal to a client — from a state the frame did not draw.
#[test]
fn the_run_key_does_nothing_while_the_keys_are_still_in_the_sidebar() {
    let host = machine_with_a_group();

    assert_eq!(
        left_after(
            &host,
            vec![Some(Signal::Down), Some(Signal::Run), Some(Signal::Leave)]
        ),
        Left::Alone,
        "`x` is the expensive one to fire by accident, which is why it is `x` \
         rather than Enter",
    );
    assert_eq!(
        left_after(
            &host,
            at_the_accounts(vec![Some(Signal::Run), Some(Signal::Leave)])
        ),
        Left::ToRun(SECOND_EMAIL.to_string()),
        "and one `→` still runs, on the Account under the cursor",
    );
}

/// A Run from the picker is the Run `perch run` performs: the client against
/// that Account's Profile, the active Account untouched, and what the client
/// said coming back as Perch's own exit code.
#[test]
fn a_run_the_view_was_left_for_hands_the_terminal_over_and_reports_what_it_said() {
    let host = machine_with_figures().with_login(client_exiting(3));
    let mut said = Vec::new();

    let ended =
        perch::commands::tui::hand_over(&host, Left::ToRun(SECOND_EMAIL.to_string()), &mut said)
            .expect("the client ran");

    assert_eq!(ended, 3, "what the client said is what a script reads");
    assert_eq!(
        String::from_utf8(said).expect("output is UTF-8"),
        "",
        "stdout belongs to the client, here as much as under `perch run`"
    );
    let said = host.notes().join("\n");
    assert!(
        said.contains(&format!("Running Claude Code as {SECOND_EMAIL}")),
        "{said}"
    );
    assert!(said.contains("in this terminal alone"), "{said}");
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(EMAIL),
        "a Run is not a Switch",
    );
}

/// The interactive view is one command among several (ADR 0011), so where
/// there is no terminal it names the ones that answer the same questions rather
/// than trying to draw.
#[test]
fn perch_tui_is_refused_where_there_is_no_terminal() {
    let host = machine_with_two_accounts().without_terminal();
    let mut written = Vec::new();

    let refusal =
        perch::commands::tui::run(&host, &mut written).expect_err("there is nothing to draw in");

    assert_eq!(
        refusal.exit_code(),
        perch::error::EXIT_INVALID,
        "a request Perch understood and refused on its own terms: {refusal}"
    );
    let said = refusal.to_string();
    assert!(said.contains("perch list"), "{said}");
    assert!(said.contains("perch status"), "{said}");
}

/// Leaving the view without choosing anything launches nothing and succeeds.
/// A picker that exited non-zero because nobody picked would make `perch tui`
/// unusable in anything that checks a status.
#[test]
fn a_view_left_alone_hands_nothing_over_and_ends_well() {
    let host = machine_with_figures();
    let mut said = Vec::new();

    let ended = perch::commands::tui::hand_over(&host, Left::Alone, &mut said)
        .expect("leaving is not a failure");

    assert_eq!(ended, 0);
    assert!(
        said.is_empty(),
        "and nothing was said about a Run that did not happen"
    );
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(EMAIL),
        "nor was anything switched"
    );
}

/// A listing taller than the frame still says what its columns are.
///
/// The header is line zero of the paragraph the rows are in, and a paragraph
/// scrolled by n skips its first n lines — so the one line that has to stay put
/// was the first one to go, as soon as the cursor moved far enough down. Five
/// Accounts on a short terminal is enough; twenty-two is enough on a standard
/// one, which is a number of subscriptions this is meant to be used with.
#[test]
fn a_listing_that_scrolls_keeps_the_row_that_says_what_the_columns_are() {
    let host = machine_with_figures();
    let mut registry = registry_of(&host);
    // Enough Accounts that the cursor has to leave the first screenful.
    let spare = registry.accounts[1].clone();
    for at in 0..8 {
        let mut another = spare.clone();
        another.identity.email = format!("spare{at}@example.com");
        registry.accounts.push(another);
    }

    let mut screen = FakeScreen::sized(
        80,
        8,
        at_the_accounts(
            std::iter::repeat_n(Some(Signal::Down), 9)
                .chain([Some(Signal::Leave)])
                .collect(),
        ),
    );
    perch::tui::browse(
        &host,
        registry,
        &mut screen,
        &mut FakeRefresher::out_for_ever(),
    )
    .expect("the TUI leaves cleanly");

    let frame = screen.last_frame();
    assert!(
        frame.contains("spare7@example.com"),
        "the Account under the cursor is on screen:\n{frame}"
    );
    assert!(
        frame.contains("Headroom"),
        "and so is the row that says what the columns are — a column somebody \
         has to scroll up to read is a column that goes unread:\n{frame}"
    );
}

// ---- What governs you, and changing it ----

/// Some keystrokes, for the navigation that gets to a row.
fn over(times: usize, signal: Signal) -> Vec<Option<Signal>> {
    std::iter::repeat_n(Some(signal), times).collect()
}

/// The frames a stepped value waits before it is written, and one more for the
/// write itself. Counted in frames rather than in milliseconds, so the fake
/// screen's "a wait nobody pressed anything in" is the whole of what drives it
/// — a debounce in milliseconds would mean giving the model a clock and every
/// test a way to advance it.
fn while_nobody_presses_anything() -> Vec<Option<Signal>> {
    vec![None, None, None]
}

/// The Settings in force for the active Account's Scope, each with where it
/// came from — written out rather than shown only as a style, so it survives a
/// pipe and a colour-blind palette.
#[test]
fn the_status_tab_says_what_governs_you_and_where_each_rule_came_from() {
    let host = machine_with_a_group();
    config_set(&host, &["work", "watcher-threshold-percent", "55"])
        .0
        .expect("the Group Overrides one Setting");

    let screen = browse(&host, at_what_governs_me(vec![Some(Signal::Leave)]));

    let said = said(screen.last_frame());
    assert!(said.contains("In force for Group `work`"), "{said}");
    assert!(
        said.contains("watcher-threshold-percent 55 set on `work`"),
        "the Override says which Scope holds it, in words rather than only as a \
         style — so it survives a pipe and a colour-blind palette: {said}"
    );
    assert!(
        said.contains("watcher-no-return true from Global"),
        "and an Inheritance says it came from Global — and shows Global's value \
         rather than a blank: {said}"
    );
}

/// Only what governs *them*. A Setting belonging to a Group somebody is not in
/// is not a rule about them.
#[test]
fn the_status_tab_shows_no_scope_the_active_account_is_not_in() {
    let host = machine_with_a_group();
    declare_group(&host, "personal");
    config_set(&host, &["personal", "strategy", "soonest-reset"])
        .0
        .expect("another Group Overrides something");

    let screen = browse(&host, at_what_governs_me(vec![Some(Signal::Leave)]));

    let said = said(screen.last_frame());
    assert!(said.contains("In force for Group `work`"), "{said}");
    assert!(
        !said.contains("personal"),
        "a Group they are not in is not a rule about them: {said}"
    );
}

/// And where somebody is in no Group, that page has an answer for them too —
/// including the Setting that decides whether they are Cycled among at all.
#[test]
fn the_status_tab_answers_for_somebody_in_no_group() {
    let host = machine_with_figures();

    let screen = browse(&host, at_what_governs_me(vec![Some(Signal::Leave)]));

    let said = said(screen.last_frame());
    assert!(said.contains("In force for The Ungrouped Scope"), "{said}");
    assert!(said.contains("cycle-ungrouped false"), "{said}");
    assert!(
        said.contains("strategy most-headroom from Global"),
        "{said}"
    );
}

#[test]
fn the_config_tab_lists_global_then_ungrouped_then_every_group() {
    let host = machine_with_a_group();
    declare_group(&host, "personal");

    let screen = browse(&host, at_the_config(vec![Some(Signal::Leave)]));

    let frame = screen.last_frame();
    // The sidebar is the left-hand column of every row, which is as wide as its
    // widest label and no wider.
    let sidebar: Vec<String> = frame
        .lines()
        .skip(1)
        .map(|line| {
            line.chars()
                .take(list::cells("+ new Group") + 3)
                .collect::<String>()
                .trim()
                .trim_start_matches("> ")
                .to_string()
        })
        .collect();
    for expected in ["Global", "Ungrouped", "personal", "work"] {
        assert!(
            sidebar.iter().any(|row| row == expected),
            "{expected} is a row of the sidebar: {sidebar:?}\n{frame}"
        );
    }
}

/// A change is written when it is made. There is no save button, and the write
/// is `perch config`'s own.
#[test]
fn one_key_flips_a_setting_and_it_is_written_at_once() {
    let host = machine_with_figures();

    browse(
        &host,
        at_the_config(
            [vec![Some(Signal::Right)], over(2, Signal::Down)]
                .concat()
                .into_iter()
                .chain([Some(Signal::Flip), Some(Signal::Leave)])
                .collect(),
        ),
    );

    assert!(
        registry_of(&host).global.settings.watcher_may_act,
        "the third row of Global's page is `watcher-may-act`"
    );
}

/// An arrow on a bool means the direction it points, not "the other one".
///
/// Both arrows read the value and inverted it, so `←` and `→` did the same
/// thing and a held key oscillated: where the Setting ended up was the parity
/// of how many repeats the terminal happened to send. A direction that means
/// one settles on an answer however long the key is held, which is the property
/// the Setting having two states and the arrows having two directions was
/// always going to give.
#[test]
fn holding_an_arrow_on_a_bool_settles_rather_than_oscillating() {
    for (signal, expected) in [(Signal::Right, true), (Signal::Left, false)] {
        let host = machine_with_figures();

        browse(
            &host,
            at_the_config(
                [vec![Some(Signal::Right)], over(2, Signal::Down)]
                    .concat()
                    .into_iter()
                    // An odd number, so a toggle and a direction disagree.
                    .chain(over(3, signal))
                    .chain([Some(Signal::Leave)])
                    .collect(),
            ),
        );

        assert_eq!(
            registry_of(&host).global.settings.watcher_may_act,
            expected,
            "three {signal:?}s on `watcher-may-act` should mean {expected}"
        );
    }
}

/// And the key named for meaning "the other one" still does, which is the half
/// a direction could have taken with it.
#[test]
fn the_flip_key_still_means_the_other_one() {
    let host = machine_with_figures();
    config_set(&host, &["watcher-may-act", "true"])
        .0
        .expect("it starts on");

    browse(
        &host,
        at_the_config(
            [vec![Some(Signal::Right)], over(2, Signal::Down)]
                .concat()
                .into_iter()
                .chain([Some(Signal::Flip), Some(Signal::Leave)])
                .collect(),
        ),
    );

    assert!(
        !registry_of(&host).global.settings.watcher_may_act,
        "Space flips what is there rather than setting a fixed value"
    );
}

/// How many times the registry was written, which is what a write costs: one
/// lock taken and given back per one of these.
fn registry_writes(host: &FakeHost) -> usize {
    host.effects()
        .iter()
        .filter(|effect| {
            matches!(effect, Effect::WrotePrivateFile(path) | Effect::WroteFile(path)
                if path.to_string_lossy().ends_with("registry.json"))
        })
        .count()
}

/// Holding an arrow key down is one write when it stops rather than one per
/// step, so a long adjustment is not a long queue of lock acquisitions.
#[test]
fn a_run_of_steps_is_one_write_once_the_keys_stop() {
    let host = machine_with_figures();
    host.forget_effects();

    browse(
        &host,
        at_the_config(
            [vec![Some(Signal::Right)], over(3, Signal::Down)]
                .concat()
                .into_iter()
                .chain(over(4, Signal::Right))
                .chain(while_nobody_presses_anything())
                .chain([Some(Signal::Leave)])
                .collect(),
        ),
    );

    assert_eq!(
        registry_of(&host).global.settings.watcher_threshold_percent,
        100,
        "four steps of five from 80"
    );
    assert_eq!(
        registry_writes(&host),
        1,
        "written once, when the keys stopped — not once per step: {:?}",
        host.effects()
    );
}

/// A Strategy steps between the readings there are, and a number's range has
/// its ends one keystroke away — the two kinds of stepping the percentage test
/// above does not cover.
#[test]
fn a_strategy_steps_between_its_readings_and_a_number_jumps_to_the_ends_of_its_range() {
    let host = machine_with_figures();

    browse(
        &host,
        at_the_config(
            [vec![Some(Signal::Right)], over(1, Signal::Down)]
                .concat()
                .into_iter()
                // The `strategy` row, then the cooldown, then its far end.
                .chain([Some(Signal::Right)])
                .chain(while_nobody_presses_anything())
                .chain(over(3, Signal::Down))
                .chain([Some(Signal::Most)])
                .chain(while_nobody_presses_anything())
                .chain([Some(Signal::Leave)])
                .collect(),
        ),
    );

    let settings = registry_of(&host).global.settings;
    assert_eq!(settings.strategy, perch::registry::Strategy::SoonestReset);
    assert_eq!(
        settings.watcher_cooldown_minutes, 10080,
        "`End` is the far end of the range the Setting itself states, so the \
         panel cannot offer a value `perch config set` would refuse"
    );
}

/// A second edit inside the debounce is a second change somebody made, not a
/// correction of the first — so displacing the deferred write writes it rather
/// than dropping it.
#[test]
fn stepping_another_row_before_the_first_settles_writes_both() {
    let host = machine_with_figures();

    browse(
        &host,
        at_the_config(
            [vec![Some(Signal::Right)], over(3, Signal::Down)]
                .concat()
                .into_iter()
                // The threshold, then straight down to the margin with no wait
                // in between.
                .chain([Some(Signal::Right), Some(Signal::Down), Some(Signal::Down)])
                .chain([Some(Signal::Right)])
                .chain(while_nobody_presses_anything())
                .chain([Some(Signal::Leave)])
                .collect(),
        ),
    );

    let settings = registry_of(&host).global.settings;
    assert_eq!(
        settings.watcher_threshold_percent, 85,
        "the first change was not lost to the second"
    );
    assert_eq!(settings.watcher_margin_percent, 15);
}

/// And walking away mid-adjustment writes it anyway: a deferred write is not a
/// save button, and nothing has to be remembered.
#[test]
fn leaving_mid_adjustment_writes_what_the_keys_had_not_got_round_to() {
    let host = machine_with_figures();

    browse(
        &host,
        at_the_config(
            [vec![Some(Signal::Right)], over(3, Signal::Down)]
                .concat()
                .into_iter()
                .chain([Some(Signal::Right), Some(Signal::Leave)])
                .collect(),
        ),
    );

    assert_eq!(
        registry_of(&host).global.settings.watcher_threshold_percent,
        85
    );
}

/// A Group's page shows the value in force even where the Group declares
/// nothing, so nobody has to go to Global to find out what is running.
#[test]
fn a_group_page_shows_an_inherited_value_rather_than_a_blank() {
    let host = machine_with_a_group();
    config_set(&host, &["watcher-cooldown-minutes", "45"])
        .0
        .expect("Global carries every Setting");

    let screen = browse(
        &host,
        at_the_config(
            over(2, Signal::Down)
                .into_iter()
                .chain([Some(Signal::Leave)])
                .collect(),
        ),
    );

    let said = said(screen.last_frame());
    assert!(said.contains("Group `work`"), "{said}");
    assert!(
        said.contains("Inherits every Setting from Global"),
        "and says so, so the dimming has a sentence to be read against: {said}"
    );
    assert!(
        said.contains("watcher-cooldown-minutes 45"),
        "with Global's value on the row rather than a blank: {said}"
    );
}

/// One key clears an Override back to Inherit, so nobody is left guessing what
/// Global's value was — and the Scope tracks Global from then on.
#[test]
fn the_clear_key_drops_an_override_and_the_group_follows_global_again() {
    let host = machine_with_a_group();
    config_set(&host, &["work", "watcher-threshold-percent", "55"])
        .0
        .expect("the Group Overrides it");

    browse(
        &host,
        at_the_config(
            [
                over(2, Signal::Down),
                vec![Some(Signal::Right), Some(Signal::Right)],
            ]
            .concat()
            .into_iter()
            .chain(over(2, Signal::Down))
            .chain([Some(Signal::Clear), Some(Signal::Leave)])
            .collect(),
        ),
    );

    let registry = registry_of(&host);
    assert_eq!(
        registry
            .group("work")
            .expect("a Group Perch holds")
            .watcher_threshold_percent,
        None,
        "the Override is gone rather than copied"
    );
    assert_eq!(
        registry
            .in_force(&perch::registry::Scope::Group("work".to_string()))
            .watcher_threshold_percent,
        80,
        "and Global's value is what is in force"
    );
}

/// The same key pressed while a stepped value is still waiting to be written.
///
/// A stepped value is deferred for a few frames so that holding an arrow is one
/// write rather than twenty, and a key that writes at once — Esc here, Space on
/// a flag — went straight out without saying anything about the edit already
/// waiting. It landed, and then the deferred one settled on top of it: the
/// Override was cleared and immediately put back, from a keystroke nobody
/// pressed twice. On screen that reads as Esc being ignored.
#[test]
fn clearing_an_override_that_was_just_stepped_leaves_it_cleared() {
    let host = machine_with_a_group();
    config_set(&host, &["work", "watcher-threshold-percent", "55"])
        .0
        .expect("the Group Overrides it");

    browse(
        &host,
        at_the_config(
            [
                over(2, Signal::Down),
                vec![Some(Signal::Right), Some(Signal::Right)],
            ]
            .concat()
            .into_iter()
            .chain(over(2, Signal::Down))
            // Stepped, and then cleared before the step has settled.
            .chain([Some(Signal::Right), Some(Signal::Clear)])
            .chain(while_nobody_presses_anything())
            .chain([Some(Signal::Leave)])
            .collect(),
        ),
    );

    assert_eq!(
        registry_of(&host)
            .group("work")
            .expect("a Group Perch holds")
            .watcher_threshold_percent,
        None,
        "the Override the user cleared stays cleared: a deferred step that \
         settles afterwards is a write they did not ask for last"
    );
}

/// At Global the same key does nothing and says why, rather than leaving
/// somebody wondering whether it silently did something.
#[test]
fn the_clear_key_at_global_does_nothing_and_says_why() {
    let host = machine_with_figures();

    let screen = browse(
        &host,
        at_the_config(
            [vec![Some(Signal::Right)], over(3, Signal::Down)]
                .concat()
                .into_iter()
                .chain([Some(Signal::Clear), Some(Signal::Leave)])
                .collect(),
        ),
    );

    let said = said(screen.last_frame());
    assert!(said.contains("nothing to clear back to"), "{said}");
    assert_eq!(
        registry_of(&host).global.settings.watcher_threshold_percent,
        80,
        "and nothing was written"
    );
}

/// The one place the panel takes typed input, because a name is the only value
/// with no natural step (ADR 0034).
#[test]
fn a_group_is_declared_by_typing_its_name() {
    let host = machine_with_figures();
    let sidebar_rows = 3; // Global, Ungrouped, and the row that declares one.

    browse(
        &host,
        at_the_config(
            over(sidebar_rows, Signal::Down)
                .into_iter()
                .chain([Some(Signal::Name)])
                .chain("spare".chars().map(|letter| Some(Signal::Typed(letter))))
                .chain([Some(Signal::Switch), Some(Signal::Leave)])
                .collect(),
        ),
    );

    assert!(
        registry_of(&host).declared_group("spare").is_some(),
        "the Group was declared"
    );
}

/// A name that collides with an existing Alias or Group is refused as it is
/// confirmed, so it is found out before anything is written.
#[test]
fn a_name_that_collides_is_refused_at_the_prompt_before_anything_is_written() {
    let host = machine_with_figures();
    set_alias(&host, "main", EMAIL).0.expect("the Alias is set");

    let screen = browse(
        &host,
        at_the_config(
            over(3, Signal::Down)
                .into_iter()
                .chain([Some(Signal::Name)])
                .chain("main".chars().map(|letter| Some(Signal::Typed(letter))))
                // The field stays open on a refusal with what was typed still
                // in it, so leaving takes an Esc and then the key that leaves.
                .chain([
                    Some(Signal::Switch),
                    Some(Signal::Clear),
                    Some(Signal::Leave),
                ])
                .collect(),
        ),
    );

    let said = said(screen.last_frame());
    assert!(said.contains("already an Alias"), "{said}");
    assert!(
        registry_of(&host).declared_group("main").is_none(),
        "and nothing was written"
    );
}

/// Ctrl-C leaves from inside the field too.
///
/// Raw mode is what makes Ctrl-C a keystroke rather than a signal, so nothing
/// else in Perch catches it — and the field dropped every key it did not
/// recognise, this one included. That left the view with no way out at all: the
/// only escape was Esc and then `q`, and a screen that does not leave when
/// Ctrl-C is pressed is the one thing the frame loop is written not to be.
///
/// The script ends at the Ctrl-C, so the fake screen fails the test if the loop
/// asks for another keystroke rather than leaving.
#[test]
fn ctrl_c_leaves_the_view_while_a_name_is_being_typed() {
    let host = machine_with_figures();

    let left = left_after(
        &host,
        at_the_config(
            over(3, Signal::Down)
                .into_iter()
                .chain([Some(Signal::Name)])
                .chain("spare".chars().map(|letter| Some(Signal::Typed(letter))))
                .chain([Some(Signal::Leave)])
                .collect(),
        ),
    );

    assert_eq!(left, Left::Alone, "and it is left for nothing else");
    assert!(
        registry_of(&host).declared_group("spare").is_none(),
        "a name abandoned rather than confirmed is not written"
    );
}

/// The third place text is typed (ADR 0034), and the surface the arrow keys
/// navigate doing what the command line can: renaming a Group.
#[test]
fn a_group_is_renamed_by_typing_over_its_name() {
    let host = machine_with_a_group();

    let screen = browse(
        &host,
        at_the_config(
            // Global, Ungrouped, then `work`.
            over(2, Signal::Down)
                .into_iter()
                .chain([Some(Signal::Name)])
                // The field opens with the name already in it, so correcting
                // four characters is four keystrokes rather than the whole name
                // typed again.
                .chain(over("work".len(), Signal::Backspace))
                .chain("day-job".chars().map(|letter| Some(Signal::Typed(letter))))
                .chain([Some(Signal::Switch), Some(Signal::Leave)])
                .collect(),
        ),
    );

    assert!(
        screen.ever_said("> work_"),
        "the prompt opens with the current name in it:\n{}",
        screen.frames().join("\n---\n")
    );
    let registry = registry_of(&host);
    assert_eq!(
        registry.declared_group("day-job"),
        Some("day-job"),
        "the Group answers to its new name"
    );
    assert_eq!(
        registry.accounts_in("day-job").len(),
        2,
        "and its Accounts came with it"
    );
    assert!(
        said(screen.last_frame()).contains("Renamed the Group `work` to `day-job`"),
        "and the write is the command, so what it printed is what the frame shows:\n{}",
        screen.last_frame()
    );
}

/// The same refusal the command would have given, so the two surfaces agree —
/// and the field stays open with what was typed still in it.
#[test]
fn a_rename_that_collides_is_refused_at_the_prompt_with_the_field_still_open() {
    let host = machine_with_a_group();
    declare_group(&host, "spare");

    let screen = browse(
        &host,
        at_the_config(
            // Global, Ungrouped, `spare`, `work` — the Groups in the order the
            // registry holds them.
            over(3, Signal::Down)
                .into_iter()
                .chain([Some(Signal::Name)])
                .chain(over("work".len(), Signal::Backspace))
                .chain("spare".chars().map(|letter| Some(Signal::Typed(letter))))
                .chain([
                    Some(Signal::Switch),
                    Some(Signal::Clear),
                    Some(Signal::Leave),
                ])
                .collect(),
        ),
    );

    assert!(
        screen.frames().iter().any(
            |frame| said(frame).contains("already a Group called `spare`")
                && frame.contains("> spare_")
        ),
        "the refusal stands with the field still open:\n{}",
        screen.frames().join("\n---\n")
    );
    let registry = registry_of(&host);
    assert!(
        registry.group("work").is_some(),
        "and nothing was written: {:?}",
        registry.groups.keys().collect::<Vec<_>>()
    );
}

/// Global is not a place and being Ungrouped is the absence of a declaration
/// rather than one somebody made (ADR 0017), so neither has a name to change.
#[test]
fn neither_global_nor_the_ungrouped_scope_is_a_group_to_rename() {
    let host = machine_with_a_group();

    for reaching in [0, 1] {
        let screen = browse(
            &host,
            at_the_config(
                over(reaching, Signal::Down)
                    .into_iter()
                    .chain([Some(Signal::Name), Some(Signal::Leave)])
                    .collect(),
            ),
        );

        let said = said(screen.last_frame());
        assert!(
            said.contains("neither is a Group somebody named"),
            "row {reaching} should say so rather than opening a field:\n{said}"
        );
    }
    assert_eq!(
        registry_of(&host).groups.len(),
        1,
        "and nothing was declared or renamed"
    );
}

/// The `+ new Group` row is not a dead end.
///
/// `←` stepped unconditionally on the Config tab's content column, which made
/// the arm that moves out unreachable. On every other row there is a value to
/// step, so the key at least did something; on this one the content column is
/// empty, so `←`, `↑`, `↓` and `Esc` all moved nothing and said nothing — the
/// silent key this panel's own rule refuses.
#[test]
fn the_new_group_row_is_not_a_dead_end() {
    let host = machine_with_a_group();
    let to_the_last_row = over(3, Signal::Down);

    let screen = browse(
        &host,
        at_the_config(
            to_the_last_row
                .into_iter()
                .chain([
                    Some(Signal::Right),
                    Some(Signal::Left),
                    Some(Signal::Up),
                    Some(Signal::Leave),
                ])
                .collect(),
        ),
    );

    // `↑` off the last row reaches `work`, whose page names it. Reached only
    // from the sidebar, so seeing it is the proof `←` got the cursor out.
    let frame = screen.last_frame();
    assert!(
        frame.contains("Group `work`"),
        "`←` left the cursor stranded in an empty column:\n{frame}"
    );
}

/// Stepping an Account out of the Scope whose page you are on must not leave
/// the cursor on a Setting.
///
/// The `group` row moves the Account out of the Group, which takes the five
/// per-Account rows off the page with it. The content cursor was clamped by row
/// *number* while the other three cursors followed what they were on, so it
/// landed on the last Setting — and because the write and the redraw are the
/// same iteration, the highlight never appeared to jump. The next repeat of a
/// held arrow then wrote an Override on a Setting nobody had touched.
#[test]
fn stepping_an_account_out_of_a_scope_does_not_leave_the_cursor_on_a_setting() {
    let host = machine_with_figures();
    declare_group(&host, "work");
    move_to_group(&host, EMAIL, "work").0.expect("it moves");
    host.forget_effects();

    let to_the_group_row = [
        over(2, Signal::Down),     // sidebar: Global, Ungrouped, work
        vec![Some(Signal::Right)], // into the Accounts column
        vec![Some(Signal::Right)], // into the content column
        over(8, Signal::Down),     // six Settings, then alias, cycling, group
    ]
    .concat();

    browse(
        &host,
        at_the_config(
            to_the_group_row
                .into_iter()
                // The first steps the Account out of the Group. The second is
                // the repeat a held key sends.
                .chain([Some(Signal::Left), Some(Signal::Left), Some(Signal::Leave)])
                .collect(),
        ),
    );

    let registry = registry_of(&host);
    assert_eq!(
        registry.account(EMAIL).and_then(|held| held.group.clone()),
        None,
        "the first press moved the Account out, which is what it is for"
    );
    assert_eq!(
        registry
            .group("work")
            .expect("the Group is still declared")
            .watcher_no_return,
        None,
        "and the second press did not write an Override on a Setting the \
         cursor was moved onto"
    );
}

/// Holding an arrow on a per-Account row is one write, like every other row.
///
/// These three wrote at once rather than waiting for the keys to stop, so a
/// held key was one registry write and one lock taken and given back per repeat
/// the terminal sent — "holding an arrow from 0 to 80 is otherwise eighty
/// writes and eighty lock acquisitions, some of which will lose the race and
/// leave a half-set value" (ADR 0034), which was the reason for the debounce
/// these rows did not use.
#[test]
fn holding_an_arrow_on_an_account_row_is_one_write_once_the_keys_stop() {
    let host = machine_with_a_group();

    let to_the_cycling_row = [
        over(2, Signal::Down),
        vec![Some(Signal::Right), Some(Signal::Right)],
        over(7, Signal::Down), // six Settings, then alias, then cycling
    ]
    .concat();

    browse(
        &host,
        at_the_config(
            to_the_cycling_row
                .into_iter()
                .chain(over(6, Signal::Left))
                .chain(while_nobody_presses_anything())
                .chain([Some(Signal::Leave)])
                .collect(),
        ),
    );

    assert_eq!(
        registry_writes(&host),
        1,
        "six repeats of one key is one change somebody made"
    );
    assert!(
        !registry_of(&host)
            .account(EMAIL)
            .expect("the Account is held")
            .enabled,
        "and it is the change they made: `←` is the low end"
    );
}

/// Naming an Account is done where the rest of the decisions about it are.
#[test]
fn an_account_is_given_an_alias_from_the_panel() {
    let host = machine_with_figures();

    browse(
        &host,
        at_the_config(
            [
                vec![Some(Signal::Down)],
                vec![Some(Signal::Right), Some(Signal::Right)],
                over(7, Signal::Down),
            ]
            .concat()
            .into_iter()
            .chain([Some(Signal::Name)])
            .chain("main".chars().map(|letter| Some(Signal::Typed(letter))))
            .chain([Some(Signal::Switch), Some(Signal::Leave)])
            .collect(),
        ),
    );

    assert_eq!(
        registry_of(&host).alias_of(EMAIL),
        Some("main"),
        "the `alias` row is the seventh on the Ungrouped page"
    );
}

/// Taking an Account out of Cycling is the one reversible per-Account decision,
/// and it is where the rest of the decisions are.
#[test]
fn an_account_is_taken_out_of_cycling_from_the_panel() {
    let host = machine_with_figures();

    browse(
        &host,
        at_the_config(
            [
                vec![Some(Signal::Down)],
                vec![Some(Signal::Right), Some(Signal::Right)],
                over(8, Signal::Down),
            ]
            .concat()
            .into_iter()
            .chain([Some(Signal::Flip), Some(Signal::Leave)])
            .collect(),
        ),
    );

    assert!(
        !registry_of(&host)
            .account(EMAIL)
            .expect("an Account Perch holds")
            .enabled,
        "the `cycling-may-choose` row is the eighth on the Ungrouped page"
    );
}

/// Declaring two Accounts interchangeable is a keystroke rather than a command.
#[test]
fn an_account_is_moved_into_another_group_from_the_panel() {
    let host = machine_with_figures();
    declare_group(&host, "work");

    browse(
        &host,
        at_the_config(
            [
                vec![Some(Signal::Down)],
                vec![Some(Signal::Right), Some(Signal::Right)],
                over(9, Signal::Down),
            ]
            .concat()
            .into_iter()
            .chain([Some(Signal::Right), Some(Signal::Leave)])
            .collect(),
        ),
    );

    assert_eq!(
        registry_of(&host)
            .account(EMAIL)
            .expect("an Account Perch holds")
            .group
            .as_deref(),
        Some("work"),
    );
}

/// The report of a write survives the write's own effect on the cursor.
///
/// `Model::wrote` set `said` and then applied the registry, and applying it
/// clears `said` whenever a cursor could not follow what it was on — the rule
/// `moving` exists for, reached the other way. A successful write does exactly
/// that when it empties the Scope on screen, so the one case where the panel had
/// most to report was the case it reported nothing at all.
///
/// One Account rather than the two `machine_with_figures` holds, because that is
/// what makes the Ungrouped Scope empty afterwards: with a second Account left
/// behind, the per-Account rows survive, the cursor follows, and nothing is
/// dropped.
#[test]
fn a_write_that_empties_the_scope_on_screen_still_says_what_it_did() {
    let host = logged_in_machine();
    declare_group(&host, "work");

    let screen = browse(
        &host,
        at_the_config(
            [
                vec![Some(Signal::Down)],
                vec![Some(Signal::Right), Some(Signal::Right)],
                over(9, Signal::Down),
                vec![Some(Signal::Right)],
                // The write lands on the debounce rather than on the way out,
                // so there is still a frame after it to read.
                while_nobody_presses_anything(),
                vec![Some(Signal::Leave)],
            ]
            .concat(),
        ),
    );

    assert_eq!(
        registry_of(&host)
            .account(EMAIL)
            .expect("an Account Perch holds")
            .group
            .as_deref(),
        Some("work"),
        "the write itself landed"
    );
    let frame = screen.last_frame();
    assert!(
        frame.contains("Moved") && frame.contains("work"),
        "and the panel says so, on the frame drawn after it: {frame}"
    );
}

/// An edit that cannot take the lock is refused, said where a failed Refresh is
/// said, and the row goes back to what was actually written — because a value
/// that was never written is not one anybody should be reading.
#[test]
fn an_edit_another_perch_holds_the_lock_against_is_refused_and_the_row_reverts() {
    let host = machine_with_figures();
    let held = perch::registry::lock(&host).expect("the other `perch` has it");

    let screen = browse(
        &host,
        at_the_config(
            [vec![Some(Signal::Right)], over(3, Signal::Down)]
                .concat()
                .into_iter()
                .chain([Some(Signal::Right)])
                .chain(while_nobody_presses_anything())
                .chain([Some(Signal::Leave)])
                .collect(),
        ),
    );

    let said = said(screen.last_frame());
    assert!(said.contains("the Perch registry lock"), "{said}");
    assert!(
        said.contains("watcher-threshold-percent 80"),
        "the row is back to what is on disk rather than showing 85, which \
         nobody wrote: {said}"
    );
    drop(held);
}

/// An edit is refused while a Refresh is out, in the same words a Switch is
/// refused in — one rule explains both, because it is one lock.
#[test]
fn an_edit_is_refused_while_a_refresh_is_out_in_the_words_a_switch_is_refused_in() {
    let host = machine_with_figures();
    let mut refresher = FakeRefresher::out_for_ever();
    let mut screen = FakeScreen::scripted(
        [
            Some(Signal::Refresh),
            Some(Signal::NextTab),
            Some(Signal::Right),
        ]
        .into_iter()
        .chain(over(2, Signal::Down))
        .chain([Some(Signal::Flip), Some(Signal::Leave)])
        .collect(),
    );

    browse_with(&host, &mut screen, &mut refresher);

    let said = said(screen.last_frame());
    assert!(
        said.contains("The Refresh you asked for is still out"),
        "{said}"
    );
    assert!(said.contains("holds Perch's own lock"), "{said}");
    assert!(
        !registry_of(&host).global.settings.watcher_may_act,
        "and nothing was written"
    );
}

/// Space flips what the row is showing, and a row showing a deferred step is
/// showing what it will become. Reading the value off disk instead, the flip
/// set what was already displayed and the write dropped the pending step as
/// same-row — so the row read the same before and after, which is the whole
/// symptom the debounce's same-row rule exists to prevent.
#[test]
fn space_on_a_stepped_flag_flips_what_is_on_screen_rather_than_what_is_on_disk() {
    let host = machine_with_figures();

    let screen = browse(
        &host,
        at_the_config(
            [
                Some(Signal::Down),
                Some(Signal::Right),
                Some(Signal::Right),
                // Steps the flag to `true` and defers the write, so the row is
                // now showing `true` while the registry still says `false`.
                Some(Signal::Right),
                // Inside the debounce, before the step has landed.
                Some(Signal::Flip),
            ]
            .into_iter()
            .chain(while_nobody_presses_anything())
            .chain([Some(Signal::Leave)])
            .collect(),
        ),
    );

    assert!(
        !registry_of(&host).global.cycle_ungrouped,
        "Space flipped the `true` the row was showing back to `false`, rather \
         than flipping the `false` on disk to the `true` already on screen:\n{}",
        screen.last_frame()
    );
}

/// `←` and `→` decide whether `↓` moves the sidebar or the listing, and whether
/// `Enter` Switches — so a frame that does not say which of the two the keys are
/// in is a frame where both acting keys fire from a state nobody can see. Both
/// columns drew themselves as the one holding them, on every frame.
#[test]
fn the_status_tab_says_which_column_the_keys_are_in() {
    let host = machine_with_figures();

    // The listing's own row rather than the bare address, which the dimmed
    // `active:` label in the header says first.
    let cursor_row = format!(">* {EMAIL}");

    let sidebar = browse(&host, vec![Some(Signal::Down), Some(Signal::Leave)]);
    assert_eq!(
        sidebar.emphasis_on("Accounts"),
        Some(Modifier::REVERSED),
        "the keys open on the sidebar:\n{}",
        sidebar.last_frame()
    );
    assert_eq!(
        sidebar.emphasis_on(&cursor_row),
        Some(Modifier::BOLD),
        "and the listing says it is where they would go:\n{}",
        sidebar.last_frame()
    );

    let listing = browse(&host, at_the_accounts(vec![Some(Signal::Leave)]));
    assert_eq!(
        listing.emphasis_on(&cursor_row),
        Some(Modifier::REVERSED),
        "one `→` and the listing has them:\n{}",
        listing.last_frame()
    );
    assert_eq!(
        listing.emphasis_on("Accounts"),
        Some(Modifier::BOLD),
        "and the sidebar has given them up:\n{}",
        listing.last_frame()
    );
}

/// The other two Status pages have no column to step into, so `→` leaves the
/// keys where the frame says they are.
///
/// `rightwards` moved them regardless, into a column only `Accounts` draws.
/// From there `↓` and `↑` drove the Accounts table's cursor — not on screen —
/// while the sidebar stopped marking its own row because the keys had left it.
/// Nothing on the frame said where they had gone, `Enter` did nothing, and only
/// `←` or `Tab` got back out.
#[test]
fn the_status_pages_with_no_column_to_enter_keep_the_keys_on_the_sidebar() {
    let host = machine_with_figures();

    // Asked of where `↓` goes rather than of a modifier, because that is the
    // symptom: the keys landing in an undrawn column showed up as arrows that
    // moved nothing anybody could see.
    // `Accounts` either way, because the sidebar clamps at its ends rather than
    // wrapping: it is the row below `Overview` and the row above `Config`.
    for (page, reaching, moves, next) in [
        ("Overview", 0, Signal::Down, "Accounts"),
        ("Config", 2, Signal::Up, "Accounts"),
    ] {
        let screen = browse(
            &host,
            [
                over(reaching, Signal::Down),
                vec![Some(Signal::Right), Some(moves), Some(Signal::Leave)],
            ]
            .concat(),
        );

        assert_eq!(
            screen.emphasis_on(next),
            Some(Modifier::REVERSED),
            "`→` on {page} leaves the keys on the sidebar, so `↓` moves it to \
             {next} rather than an Accounts cursor no frame draws:\n{}",
            screen.last_frame()
        );
    }
}

/// A value the model refuses is refused in the words the command would have
/// used, because it *is* the command.
#[test]
fn the_ungrouped_page_carries_the_setting_that_gates_it() {
    let host = machine_with_figures();

    let screen = browse(
        &host,
        at_the_config(
            [Some(Signal::Down), Some(Signal::Right), Some(Signal::Right)]
                .into_iter()
                .chain([Some(Signal::Flip), Some(Signal::Leave)])
                .collect(),
        ),
    );

    let said = said(screen.last_frame());
    assert!(said.contains("cycle-ungrouped"), "{said}");
    assert!(
        registry_of(&host).global.cycle_ungrouped,
        "the first row of the Ungrouped page is the Setting that decides whether \
         those Accounts are Cycled among at all (ADR 0017)"
    );
    assert!(
        said.contains("now Cycles among the other ungrouped Accounts"),
        "in the words `perch config set` would have used: {said}"
    );
}

/// The one place the layering is deliberately not uniform, said where somebody
/// would otherwise read the watcher's Settings and believe something is running
/// that is not (ADR 0017).
#[test]
fn the_ungrouped_page_says_the_watcher_settings_are_not_in_force_until_the_gate_is_open() {
    let host = machine_with_figures();

    let screen = browse(
        &host,
        at_the_config(vec![Some(Signal::Down), Some(Signal::Leave)]),
    );

    let shut = said(screen.last_frame());
    assert!(
        shut.contains("The watcher Settings are not in force here"),
        "{shut}"
    );
    assert!(shut.contains("`cycle-ungrouped` is off"), "{shut}");

    config_set(&host, &["cycle-ungrouped", "true"])
        .0
        .expect("the gate opens");
    let screen = browse(
        &host,
        at_the_config(vec![Some(Signal::Down), Some(Signal::Leave)]),
    );

    assert!(
        !said(screen.last_frame()).contains("not in force"),
        "{}",
        screen.last_frame()
    );
}

/// The Scope's Accounts are a column of their own, so who is affected by these
/// Settings is on the page beside them.
#[test]
fn a_scopes_page_lists_the_accounts_that_scope_governs() {
    let host = machine_with_figures();
    declare_group(&host, "work");
    move_to_group(&host, EMAIL, "work").0.expect("it moves");

    // The Ungrouped page: the Account left in no Group, and not the other.
    let screen = browse(
        &host,
        at_the_config(vec![Some(Signal::Down), Some(Signal::Leave)]),
    );
    let frame = screen.last_frame();
    assert!(frame.contains(SECOND_EMAIL), "{frame}");
    assert!(
        !frame.lines().skip(1).any(|line| line.contains(EMAIL)),
        "the Account in a Group is not governed by this Scope\n{frame}"
    );

    // And the Group's page: the other way round.
    let screen = browse(
        &host,
        at_the_config(
            over(2, Signal::Down)
                .into_iter()
                .chain([Some(Signal::Leave)])
                .collect(),
        ),
    );
    let frame = screen.last_frame();
    assert!(frame.contains(EMAIL), "{frame}");
    assert!(
        !frame
            .lines()
            .skip(1)
            .any(|line| line.contains(SECOND_EMAIL)),
        "{frame}"
    );
}

/// An Account selected in that column shows its facts, which is what says what
/// you are looking at before you change any of them.
#[test]
fn an_account_selected_in_that_column_shows_its_facts() {
    let host = machine_with_figures();
    set_alias(&host, "main", EMAIL).0.expect("it is named");
    quarantine(&host, EMAIL);

    // Wide enough that the values are read rather than cut: what is under test
    // is which facts are on the page, not how a narrow terminal wraps them.
    let mut screen = FakeScreen::sized(
        100,
        24,
        at_the_config(vec![
            Some(Signal::Down),
            Some(Signal::Right),
            Some(Signal::Right),
            Some(Signal::Leave),
        ]),
    );
    browse_with(&host, &mut screen, &mut FakeRefresher::out_for_ever());

    let said = said(screen.last_frame());
    assert!(said.contains("alias main"), "{said}");
    assert!(said.contains("cycling-may-choose true"), "{said}");
    assert!(said.contains("group none"), "{said}");
    assert!(said.contains("quarantine renewal-rejected"), "{said}");
}

/// The three questions the `Status` tab answers are on screen as the rows they
/// are, so the tab can be navigated by eye.
#[test]
fn the_status_sidebar_names_the_three_questions_it_answers() {
    let host = machine_with_figures();

    let screen = browse(&host, vec![Some(Signal::Leave)]);

    let frame = screen.last_frame();
    for row in ["Overview", "Accounts", "Config"] {
        assert!(
            frame
                .lines()
                .any(|line| line.trim_start_matches("> ").trim_start().starts_with(row)),
            "{row} is a row of the sidebar\n{frame}"
        );
    }
}

/// A Setting a Scope Overrides reads as that Scope's own, and one it Inherits
/// reads as Global's, on the page where a cursor is the way round.
#[test]
fn a_group_page_says_how_many_settings_it_declares_of_its_own() {
    let host = machine_with_a_group();
    config_set(&host, &["work", "strategy", "soonest-reset"])
        .0
        .expect("one Override");
    config_set(&host, &["work", "watcher-no-return", "false"])
        .0
        .expect("and a second");

    let screen = browse(
        &host,
        at_the_config(
            over(2, Signal::Down)
                .into_iter()
                .chain([Some(Signal::Leave)])
                .collect(),
        ),
    );

    let said = said(screen.last_frame());
    assert!(said.contains("Overrides 2 Settings"), "{said}");
    assert!(said.contains("strategy soonest-reset"), "{said}");
    assert!(
        said.contains("watcher-threshold-percent 80"),
        "and the Inherited ones show Global's value: {said}"
    );
}

/// A deferred write is held rather than taken while a Refresh is out — that
/// Refresh holds Perch's own lock, and a write on the frame loop would sit
/// waiting on it with the screen frozen — and it is held rather than dropped,
/// so leaving still writes it.
#[test]
fn a_deferred_write_waits_out_a_refresh_and_is_not_lost_to_it() {
    let host = machine_with_figures();
    let mut refresher = FakeRefresher::out_for_ever();
    let doing = at_the_config(
        [vec![Some(Signal::Right)], over(3, Signal::Down)]
            .concat()
            .into_iter()
            .chain([Some(Signal::Right), Some(Signal::Refresh)])
            .chain(while_nobody_presses_anything())
            .chain([Some(Signal::Leave)])
            .collect(),
    );
    let scripted = doing.len();
    let mut screen = FakeScreen::scripted(doing);

    browse_with(&host, &mut screen, &mut refresher);

    assert_eq!(
        screen.frames().len(),
        scripted,
        "the loop went on drawing rather than blocking on the lock the Refresh \
         is holding"
    );
    assert_eq!(
        registry_of(&host).global.settings.watcher_threshold_percent,
        85,
        "and the edit was held rather than dropped, so leaving wrote it"
    );
    assert!(
        refresher.was_waited_for(),
        "behind the Refresh rather than racing it"
    );
}
