//! `perch tui` — the frame loop, driven with no terminal.
//!
//! The loop under test is the real one. What is faked is the terminal it draws
//! in ([`FakeScreen`], which keeps every frame as text) and the Refresh it can
//! ask for ([`FakeRefresher`], which answers when the test says so) — so these
//! say what somebody pressed and then read what they would have seen.

mod common;

use common::*;

use chrono::Duration;
use perch::host::{FakeHost, Host};
use perch::registry::Registry;
use perch::tui::fake::{FakeRefresher, FakeScreen};
use perch::tui::refresh::Refreshed;
use perch::tui::{Left, Signal};

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
    assert!(frame.contains("Accounts"), "{frame}");
    assert!(frame.contains("Utilization"), "{frame}");
    assert!(frame.contains("q  quit"), "{frame}");
    assert!(frame.contains("Tab  view"), "{frame}");
    assert!(frame.contains("r  refresh"), "{frame}");
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

    let screen = browse(&host, vec![Some(Signal::Leave)]);

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
            .any(|line| line.starts_with(">* ") && line.contains(EMAIL)),
        "{frame}"
    );
}

/// ADR 0015: every figure is shown with its age, so a stale number is visibly
/// stale rather than quietly wrong.
#[test]
fn the_age_of_every_figure_is_on_the_utilization_view() {
    let host = machine_with_figures();

    let screen = browse(&host, vec![Some(Signal::NextTab), Some(Signal::Leave)]);

    let frame = screen.last_frame();
    for said in [EMAIL, SECOND_EMAIL, "5-hour", "42%", "7%", "(as of 4m ago)"] {
        assert!(frame.contains(said), "{said} is missing from\n{frame}");
    }
    assert_eq!(
        frame.matches("as of").count(),
        4,
        "each Account's Headroom and each of its Quota Windows carries its own \
         age\n{frame}"
    );
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

    let screen = browse(&host, vec![Some(Signal::NextTab), Some(Signal::Leave)]);

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
    for row in [row("5-hour"), row("7-day")] {
        assert!(row.contains("(as of 4m ago)"), "{row}");
    }
}

/// ADR 0012: an Account is only ever as free as its fullest window, so the one
/// figure it is compared on comes from that window — and names it, so the claim
/// can be checked against the rows underneath rather than taken on trust.
#[test]
fn each_account_shows_the_headroom_its_most_constrained_window_leaves() {
    let host = machine_with_figures();
    observed(
        &host,
        EMAIL,
        vec![window("5-hour", 4.0), window("7-day", 95.0)],
    );

    let screen = browse(&host, vec![Some(Signal::NextTab), Some(Signal::Leave)]);

    let frame = screen.last_frame();
    // Its own block's heading, and not the tab bar's "active:" label.
    let heading = frame
        .lines()
        .find(|line| line.contains(EMAIL) && !line.contains("active:"))
        .unwrap_or_else(|| panic!("{EMAIL} heads its own block in\n{frame}"));
    assert!(heading.contains("Headroom 5%"), "{heading}");
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

    let screen = browse(&host, vec![Some(Signal::NextTab), Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(frame.contains("Group `work`"), "{frame}");
    assert!(
        frame.contains("Reserve: 2 of 2 Accounts have Headroom, the best 93% left (as of 4m ago)"),
        "{frame}"
    );
}

/// Summing or averaging percentages across Accounts produces a number that
/// looks quantitative, is not, and is exactly the kind of number people plan
/// around. So every figure a Group's rows quote is one an Account reported.
#[test]
fn no_figure_on_the_tab_is_one_no_account_reported() {
    let host = machine_with_a_group();

    let screen = browse(&host, vec![Some(Signal::NextTab), Some(Signal::Leave)]);

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

    let screen = browse(&host, vec![Some(Signal::NextTab), Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(
        frame.contains("5-hour emptiest  96% used across 2 Accounts (as of 4m ago)"),
        "the Group is nearly out on its five-hour window\n{frame}"
    );
    assert!(
        frame.contains("7-day  emptiest  12% used across 2 Accounts (as of 4m ago)"),
        "and fine on its seven-day one\n{frame}"
    );
}

/// ADR 0017: being in no Group is the absence of a declaration that Accounts
/// are interchangeable. A Reserve over Accounts nobody has said are
/// interchangeable would be a figure about a set that is not one.
#[test]
fn the_accounts_in_no_group_get_no_reserve_until_cycling_may_choose_them() {
    let host = machine_with_figures();

    let screen = browse(&host, vec![Some(Signal::NextTab), Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(frame.contains("In no Group"), "{frame}");
    assert!(!frame.contains("Reserve"), "{frame}");
    assert!(
        frame.contains("only moves between these when you say it may"),
        "and it says what would have to change\n{frame}"
    );

    config_set(&host, &["cycle-ungrouped", "true"])
        .0
        .expect("they are declared interchangeable");
    let screen = browse(&host, vec![Some(Signal::NextTab), Some(Signal::Leave)]);

    assert!(
        screen
            .last_frame()
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

    let screen = browse(&host, vec![Some(Signal::NextTab), Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(
        frame.contains("Reserve: 1 of 1 Account has Headroom, the best 58% left"),
        "{frame}"
    );
    assert!(
        frame.contains("1 disabled, so nothing Cycles to it."),
        "{frame}"
    );
    assert!(
        frame.contains(SECOND_EMAIL),
        "and it is still listed with its own figures\n{frame}"
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
    let mut screen = FakeScreen::scripted(vec![
        Some(Signal::NextTab),
        Some(Signal::Refresh),
        None,
        Some(Signal::Leave),
    ]);

    browse_with(&host, &mut screen, &mut refresher);

    assert!(
        screen.frames()[1].contains("the best 93% left (as of 4m ago)"),
        "the Reserve before the Refresh\n{}",
        screen.frames()[1]
    );
    let frame = screen.last_frame();
    assert!(
        frame.contains("the best 97% left (as of just now)"),
        "the Reserve moved with the figures it is made of\n{frame}"
    );
    assert!(
        frame.contains("5-hour emptiest   3% used across 2 Accounts (as of just now)"),
        "{frame}"
    );

    // And one that could not read anything leaves every figure with the age it
    // had, rather than the Group's rows emptying because Anthropic was busy.
    let mut failed = FakeRefresher::answering(
        Refreshed::nothing_read(vec!["overflow@example.com: no answer".to_string()]),
        0,
    );
    let mut screen = FakeScreen::scripted(vec![
        Some(Signal::NextTab),
        Some(Signal::Refresh),
        Some(Signal::Leave),
    ]);

    browse_with(&host, &mut screen, &mut failed);

    let frame = screen.last_frame();
    assert!(frame.contains("no answer"), "{frame}");
    assert!(
        frame.contains("the best 93% left (as of 4m ago)"),
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

    let screen = browse(&host, vec![Some(Signal::NextTab), Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(
        frame.contains("Reserve: none of 2 Accounts have Headroom (2 exhausted)"),
        "{frame}"
    );
    assert!(
        frame.contains("Read 4m ago at the oldest."),
        "a count read from cache says how old the readings behind it are\n{frame}"
    );
    assert!(
        frame.contains("Headroom exhausted  (as of 4m ago)"),
        "{frame}"
    );
}

/// Fill and room are both percentages, and two of them an inch apart are told
/// apart by a word rather than by context.
#[test]
fn a_figure_says_whether_it_is_room_left_or_quota_used() {
    let host = machine_with_a_group();

    let screen = browse(&host, vec![Some(Signal::NextTab), Some(Signal::Leave)]);

    let frame = screen.last_frame();
    assert!(frame.contains("the best 93% left"), "{frame}");
    assert!(frame.contains("emptiest   7% used"), "{frame}");
    assert!(
        frame.contains("5-hour   7% used  no reset time cached"),
        "and the Account's own row says it too\n{frame}"
    );
}

/// "No figure" and "plenty of room" are opposite pieces of advice.
#[test]
fn an_account_never_observed_says_so_rather_than_showing_zero() {
    let host = machine_with_two_accounts();

    let screen = browse(&host, vec![Some(Signal::NextTab), Some(Signal::Leave)]);

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
        frames[1].contains("as of"),
        "the second view\n{}",
        frames[1]
    );
    assert!(
        !frames[2].contains("as of"),
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
    let mut screen = FakeScreen::sized(46, 10, vec![Some(Signal::Leave)]);

    browse_with(&host, &mut screen, &mut FakeRefresher::out_for_ever());

    let bar = screen
        .last_frame()
        .lines()
        .next()
        .expect("a frame")
        .to_string();
    assert!(bar.contains("Accounts"), "{bar}");
    assert!(bar.contains("Utilization"), "{bar}");
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
        screen.last_frame().contains("as of"),
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
    let mut screen = FakeScreen::scripted(vec![
        Some(Signal::NextTab),
        Some(Signal::Refresh),
        None,
        Some(Signal::Leave),
    ]);

    browse_with(&host, &mut screen, &mut refresher);

    assert!(
        screen.frames()[1].contains("42% used  no reset time cached  (as of 4m ago)"),
        "the figure before the Refresh\n{}",
        screen.frames()[1]
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
    let mut screen = FakeScreen::scripted(vec![
        Some(Signal::NextTab),
        Some(Signal::Refresh),
        Some(Signal::Leave),
    ]);

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

    let screen = browse(&host, vec![Some(Signal::Leave)]);

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

    let screen = browse(&host, vec![Some(Signal::Leave)]);

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
    let screen = browse(&host, vec![Some(Signal::Switch), None, Some(Signal::Leave)]);

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
            .any(|line| line.starts_with(">* ") && line.contains(SECOND_EMAIL)),
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

    let screen = browse(&host, vec![Some(Signal::Switch), None, Some(Signal::Leave)]);

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
        vec![
            Some(Signal::Refresh),
            Some(Signal::Switch),
            Some(Signal::Leave),
        ],
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
        vec![
            Some(Signal::Down),
            Some(Signal::Switch),
            Some(Signal::Leave),
        ],
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
            vec![Some(Signal::Down), Some(key), Some(Signal::Leave)],
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
        left_after(&host, vec![Some(Signal::Down), Some(Signal::Run)]),
        Left::ToRun(EMAIL.to_string()),
    );
    assert_eq!(
        left_after(&host, vec![Some(Signal::Leave)]),
        Left::Alone,
        "`q` leaves and launches nothing",
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
        6,
        std::iter::repeat_n(Some(Signal::Down), 9)
            .chain([Some(Signal::Leave)])
            .collect(),
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

/// Selecting an Account below the fold shows its figures, not just its name.
///
/// The scroll brings one named line into view with what is around it. On the
/// Accounts tab that line is the row, which is the whole of the content. On the
/// Utilization tab each Account's Quota Window rows come *after* its heading, so
/// naming the heading put every figure below the bottom edge: the selected
/// Account rendered as a bare name with its Utilization scrolled off, on the one
/// tab whose whole purpose is the figures. The last row of the block is what is
/// named instead, which keeps the block together.
#[test]
fn moving_down_the_utilization_view_keeps_the_selected_accounts_figures_on_screen() {
    let host = machine_with_figures();
    // Short enough that the second Account's block cannot share the screen with
    // the first, so the view has to scroll to reach it at all.
    let mut screen = FakeScreen::sized(
        80,
        7,
        vec![
            Some(Signal::NextTab),
            Some(Signal::Down),
            Some(Signal::Leave),
        ],
    );

    browse_with(&host, &mut screen, &mut FakeRefresher::out_for_ever());

    let frame = screen.last_frame();
    assert!(
        frame.contains(SECOND_EMAIL),
        "the selected Account is on screen:\n{frame}"
    );
    assert!(
        frame.contains("7% used"),
        "and so is the figure that is the whole reason for this view:\n{frame}"
    );
}
