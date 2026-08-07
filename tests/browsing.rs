//! `perch tui` — the frame loop, driven with no terminal.
//!
//! The loop under test is the real one. What is faked is the terminal it draws
//! in ([`FakeScreen`], which keeps every frame as text) and the Refresh it can
//! ask for ([`FakeRefresher`], which answers when the test says so) — so these
//! say what somebody pressed and then read what they would have seen.

mod common;

use common::*;

use perch::host::{FakeHost, Host};
use perch::registry::Registry;
use perch::tui::Signal;
use perch::tui::fake::{FakeRefresher, FakeScreen};
use perch::tui::refresh::Refreshed;

/// Opens the TUI on this machine and does these things at it, with a Refresh
/// that never comes back — which is what one looks like for as long as it is
/// out, and what the ordinary case never asks for at all.
fn browse(host: &FakeHost, doing: Vec<Option<Signal>>) -> FakeScreen {
    let mut screen = FakeScreen::scripted(doing);
    browse_with(host, &mut screen, &mut FakeRefresher::out_for_ever());
    screen
}

/// The same, for the tests that are about what the Refresh does.
fn browse_with(host: &FakeHost, screen: &mut FakeScreen, refresher: &mut FakeRefresher) {
    let registry = perch::adopt::ensure_adopted(host, &mut std::io::sink())
        .expect("the machine has a login to adopt");
    perch::tui::browse(host, registry, screen, refresher).expect("the TUI leaves cleanly");
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

/// A machine with two Accounts, both with figures four minutes old.
fn machine_with_figures() -> FakeHost {
    let host = machine_with_two_accounts();
    observed(&host, EMAIL, vec![window("5-hour", 42.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 7.0)]);
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
        2,
        "both figures carry their own age\n{frame}"
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
        screen.frames()[1].contains("42%  (as of 4m ago)"),
        "the figure before the Refresh\n{}",
        screen.frames()[1]
    );
    let frame = screen.last_frame();
    assert!(frame.contains("3%"), "{frame}");
    assert!(frame.contains("(as of just now)"), "{frame}");
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

/// The interactive view is one command among several (ADR 0011), so where
/// there is no terminal it names the ones that answer the same questions rather
/// than trying to draw.
#[test]
fn perch_tui_is_refused_where_there_is_no_terminal() {
    let host = machine_with_two_accounts().without_terminal();
    let mut written = Vec::new();

    let refusal =
        perch::commands::tui::run(&host, &mut written).expect_err("there is nothing to draw in");

    let said = refusal.to_string();
    assert!(said.contains("perch list"), "{said}");
    assert!(said.contains("perch status"), "{said}");
}
