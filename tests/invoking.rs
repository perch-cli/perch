//! Surface: what the built binary accepts, what it prints, and what it exits
//! with (ADR 0044).
//!
//! The one suite that runs `perch` as a process. Every other binary in `tests/`
//! drives the command code in this process against `host::fake`, which is
//! faster, hermetic on every platform, and names the assertion that broke. What
//! cannot be reached that way is the surface: which dispatch arm a parsed
//! command line reaches, and whether the code `ended_as` computed survives to a
//! shell — `main` cannot be called from a test.
//!
//! So this suite claims the surface and never the behaviour behind it. The line
//! is operational: **if a case needs a real Claude Code installed, it has
//! crossed.** Everything here runs against a scratch home on a machine with no
//! Claude Code, no keychain consulted and no network answered, which is why
//! `switch`, `add`, `run`, `relogin` and `watcher run` are absent. Those arms
//! probe, the probe is the boundary marker rather than an obstacle, and a stub
//! `claude` on `PATH` would be working around it. Behaviour stays with the
//! fakes.
//!
//! The parser is not asserted here either. What parses and what does not is
//! claimed by the table in `src/main.rs` — a sharper claim than an exit code
//! and a clap message that is not Perch's to hold still, and one that spends no
//! process. What is claimed here is what the process does with a line that got
//! through, and that a line which did not ends at 2.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use perch::error::{
    EXIT_CONFLICT, EXIT_INVALID, EXIT_NOT_FOUND, EXIT_NOT_UNDERSTOOD, EXIT_NOTHING_TO_DO, EXIT_OK,
};
use perch::probe::Identity;
use perch::registry::{Account, Active, Overrides, Registry};

/// The Account every scratch machine holds, and the Group declared beside it.
const SOMEONE: &str = "someone@example.com";
const GROUP: &str = "work";

/// What one run of the binary said, and what it ended as.
struct Ran {
    code: i32,
    out: String,
    err: String,
}

/// Runs the built binary — the one this test was compiled beside, which Cargo
/// names for every integration test — against a scratch machine.
fn perch(machine: &Scratch, line: &[&str]) -> Ran {
    let ran = Command::new(env!("CARGO_BIN_EXE_perch"))
        .args(line)
        .env("PERCH_HOME", machine.home())
        .env("CLAUDE_CONFIG_DIR", machine.claude())
        .output()
        .expect("the binary the build produced runs");
    Ran {
        // A process this suite ran that has no code was killed by a signal,
        // which is news rather than a failed assertion.
        code: ran
            .status
            .code()
            .expect("the binary exited rather than being killed"),
        out: String::from_utf8(ran.stdout).expect("output is UTF-8"),
        err: String::from_utf8(ran.stderr).expect("output is UTF-8"),
    }
}

/// A machine of Perch's own: the two directories every command reads its state
/// out of, pointed somewhere nothing else will look.
///
/// `PERCH_HOME` and `CLAUDE_CONFIG_DIR` are ordinary user-facing variables —
/// `service.rs` carries both into a generated unit file — so isolation here
/// costs no test-only escape hatch and no `#[cfg]`. It is what somebody who
/// moved both directories deliberately would get.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    /// A machine Perch has never run on: no home directory at all, which is
    /// where `perch holdings purge` finds nothing to give back.
    ///
    /// Named for the test that made it, under this process, so two of these
    /// never collide however many tests run at once.
    fn untouched(by: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!("perch-invoking-{}-{by}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("claude")).expect("a scratch machine can be made");
        Scratch { root }
    }

    /// The ordinary machine these tests drive: one Account, active and a Cycle
    /// candidate, and one Group declared holding nothing.
    ///
    /// Written rather than adopted, because adoption is what asks the machine
    /// what Claude Code is installed. A registry already there is what every
    /// command below reads, so none of them asks.
    fn holding_an_account(by: &str) -> Scratch {
        let machine = Scratch::untouched(by);

        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: SOMEONE.to_string(),
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
        registry.active = Active::Settled(SOMEONE.to_string());
        registry
            .groups
            .insert(GROUP.to_string(), Overrides::default());

        fs::create_dir_all(machine.home()).expect("a scratch home can be made");
        fs::write(
            machine.home().join("registry.json"),
            serde_json::to_string(&registry).expect("a registry is a document"),
        )
        .expect("the registry can be written");
        machine
    }

    fn home(&self) -> PathBuf {
        self.root.join("perch")
    }

    fn claude(&self) -> PathBuf {
        self.root.join("claude")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Every command the binary dispatches, as `--help` lists them.
///
/// Fifteen names, because a command is placed by the noun it is about and the
/// Account is the one left unsaid (ADR 0047). The ten that elide it, `perch`
/// itself, and the four nouns that are written: `config`, `group`, `holdings`
/// and `watcher`.
const COMMANDS: [&str; 15] = [
    "add", "alias", "config", "disable", "enable", "group", "holdings", "relogin", "remove", "run",
    "list", "switch", "status", "upgrade", "watcher",
];

/// The bare question, answered before the parser and exactly as the Homebrew
/// formula's test block asserts on it.
///
/// One line and no more: the upgrade notice is for a person at a terminal, and
/// a pipe is not one — which is what keeps a suite that must answer no network
/// from asking about a Release on every case.
#[test]
fn the_version_question_is_answered_by_the_process_in_one_line() {
    let machine = Scratch::holding_an_account("version");

    for asked in [["--version"], ["-V"]] {
        let ran = perch(&machine, &asked);

        assert_eq!(ran.code, EXIT_OK, "{asked:?}");
        assert_eq!(
            ran.out,
            format!("perch {}\n", env!("CARGO_PKG_VERSION")),
            "`perch <version>` and nothing underneath it"
        );
    }
}

/// The one rendering nothing else has ever produced. Every command is on it,
/// because a command missing from `--help` is one nobody finds.
#[test]
fn help_names_every_command_the_binary_dispatches() {
    let machine = Scratch::holding_an_account("help");

    let ran = perch(&machine, &["--help"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);

    let usage = ran
        .out
        .lines()
        .find(|line| line.starts_with("Usage:"))
        .unwrap_or_else(|| panic!("the help says how it is used:\n{}", ran.out));

    // The program, and that a command is wanted — rather than the whole line.
    // clap names the program from `argv[0]`, so a Windows `perch --help` says
    // `perch.exe`, which is the name the file on disk has. Which of its own
    // names the binary was reached by is the shell's business rather than
    // Perch's, and this suite is the first thing ever to render this help on
    // all three platforms.
    assert!(usage.contains("perch"), "{usage}");
    assert!(
        usage.contains("<COMMAND>"),
        "and that a command is wanted: {usage}"
    );

    for command in COMMANDS {
        // The line it is listed on rather than anywhere in the rendering: the
        // prose above the list says `run` and `add` too.
        assert!(
            ran.out.lines().any(|line| line
                .trim_start()
                .strip_prefix(command)
                .is_some_and(|rest| rest.starts_with(' '))),
            "`{command}` is missing from the help:\n{}",
            ran.out
        );
    }
}

/// The claim no in-process test can make: `ended_as` computes a code, and that
/// code is what a shell reads. `main` cannot be called, so the only way to
/// assert this is to be the shell.
///
/// Every line here is a real dispatch arm over real bytes, and between them
/// they carry the codes the reachable commands answer with.
#[test]
fn the_code_a_command_ended_as_is_what_the_process_exits_with() {
    let machine = Scratch::holding_an_account("codes");

    for (line, code) in [
        (&["list"][..], EXIT_OK),
        (&["disable", "nobody@example.com"], EXIT_NOT_FOUND),
        (&["group", "move", SOMEONE, "nowhere"], EXIT_NOT_FOUND),
        (&["group", "add", GROUP], EXIT_CONFLICT),
        (&["alias", GROUP, SOMEONE], EXIT_CONFLICT),
        (
            &["config", "set", "watcher-threshold-percent", "500"],
            EXIT_INVALID,
        ),
        (
            &["config", "set", "nothing-of-the-sort", "true"],
            EXIT_INVALID,
        ),
    ] {
        let ran = perch(&machine, line);

        assert_eq!(
            ran.code,
            code,
            "`perch {}` ends as {code}:\n{}{}",
            line.join(" "),
            ran.out,
            ran.err
        );
    }
}

/// A machine Perch is holding nothing on, which is the one place a reachable
/// command answers "there was nothing to do".
#[test]
fn a_machine_perch_holds_nothing_on_has_nothing_to_purge() {
    let machine = Scratch::untouched("purge");

    let ran = perch(&machine, &["holdings", "purge", "--yes"]);

    assert_eq!(ran.code, EXIT_NOTHING_TO_DO, "{}{}", ran.out, ran.err);
    assert!(
        ran.err.contains("nothing to give back"),
        "and says so rather than making the home it was asked about:\n{}",
        ran.err
    );
    assert!(
        !machine.home().exists(),
        "a Purge that made a home to report there was none took away nothing"
    );
}

/// Where a refusal is said. Standard output is what a script parses, so a
/// failure goes to the other stream — asserted here at the process, where the
/// two are genuinely separate file descriptors rather than one buffer a test
/// handed in.
#[test]
fn a_refusal_is_said_on_stderr_and_never_on_the_stream_a_script_reads() {
    let machine = Scratch::holding_an_account("streams");

    let ran = perch(&machine, &["disable", "nobody@example.com"]);

    assert_eq!(ran.code, EXIT_NOT_FOUND, "{}{}", ran.out, ran.err);
    assert!(
        ran.out.is_empty(),
        "nothing is said on standard output:\n{}",
        ran.out
    );
    assert!(
        ran.err.contains("nobody@example.com"),
        "and the refusal names what could not be found:\n{}",
        ran.err
    );
}

/// A word that is not a command, and the narrowings the table in `main.rs`
/// refuses. What is claimed here is only that a refused line ends the process
/// at 2 with nothing on standard output — which of them clap refuses and in
/// what words is the parser's, and is asserted there.
#[test]
fn a_line_the_parser_refuses_ends_the_process_at_two() {
    let machine = Scratch::holding_an_account("refused");

    for line in [
        &["frobnicate"][..],
        &[
            "holdings",
            "export",
            "/tmp/perch.age",
            "--passphrase",
            "hunter2",
        ],
        &["holdings", "import", "/tmp/perch.age", "--force"],
        &["holdings", "purge", "--group", GROUP],
        &["upgrade", "--json"],
    ] {
        let ran = perch(&machine, line);

        assert_eq!(
            ran.code,
            EXIT_NOT_UNDERSTOOD,
            "`perch {}` is refused:\n{}{}",
            line.join(" "),
            ran.out,
            ran.err
        );
        assert!(
            ran.out.is_empty(),
            "`perch {}` reached no command:\n{}",
            line.join(" "),
            ran.out
        );
    }
}

/// The refusal Perch writes itself, which is read off the words before the
/// parser sees them — so this asserts the order those two run in, which only a
/// process shows: clap would have refused this line in its own words.
#[test]
fn a_flag_that_could_be_either_is_refused_before_the_parser() {
    let machine = Scratch::holding_an_account("separator");

    let ran = perch(&machine, &["run", "dev", "--resume"]);

    assert_eq!(ran.code, EXIT_NOT_UNDERSTOOD, "{}{}", ran.out, ran.err);
    assert!(
        ran.err.contains("perch run dev -- --resume"),
        "and names the line that would have worked:\n{}",
        ran.err
    );
}

/// The listing arm, and the flag that is the only thing on it. A table for a
/// person and a document for a script are the two renderings ADR 0011 requires
/// to both exist.
#[test]
fn the_listing_arm_renders_a_table_and_the_json_flag_a_document() {
    let machine = Scratch::holding_an_account("list");

    let ran = perch(&machine, &["list"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    assert!(ran.out.contains(SOMEONE), "{}", ran.out);
    assert!(ran.out.contains("Utilization"), "{}", ran.out);

    let ran = perch(&machine, &["list", "--json"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    let document: serde_json::Value =
        serde_json::from_str(&ran.out).unwrap_or_else(|_| panic!("a document:\n{}", ran.out));
    assert_eq!(document["active_account"], SOMEONE, "{}", ran.out);
}

/// The status arm, whose args struct carries three booleans of one type: a
/// `--group` that arrived where `--refresh` was read would fetch, and one that
/// arrived where `--json` was read would answer a script.
///
/// `--refresh` is the one flag this suite cannot press — it is the only thing
/// in Perch that touches the network — so what is claimed is that neither other
/// flag is it.
#[test]
fn the_status_arm_answers_about_one_account_and_each_flag_is_its_own() {
    let machine = Scratch::holding_an_account("status");

    let ran = perch(&machine, &["status"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    assert!(ran.out.contains(SOMEONE), "{}", ran.out);
    assert!(
        !ran.out.contains("In no Group"),
        "the bare question is about one Account:\n{}",
        ran.out
    );

    let ran = perch(&machine, &["status", "--group"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    assert!(
        ran.out.contains("In no Group"),
        "and `--group` widens it to the Scope you could land in:\n{}",
        ran.out
    );

    let ran = perch(&machine, &["status", "--json"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    let document: serde_json::Value =
        serde_json::from_str(&ran.out).unwrap_or_else(|_| panic!("a document:\n{}", ran.out));
    assert_eq!(document["active"]["email"], SOMEONE, "{}", ran.out);
}

/// The three Config arms, which are told apart by what they do to a Scope's
/// Override: a `set` that reached `get` would change nothing, and an `unset`
/// that reached `set` would leave the Override it was asked to clear.
#[test]
fn the_config_arms_read_a_setting_change_one_and_clear_one() {
    let machine = Scratch::holding_an_account("config");

    let ran = perch(&machine, &["config", "get"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    assert!(
        ran.out.contains("watcher-threshold-percent 80"),
        "{}",
        ran.out
    );

    let ran = perch(
        &machine,
        &["config", "set", GROUP, "watcher-may-act", "true"],
    );

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);

    let ran = perch(&machine, &["config", "get", GROUP]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    assert!(
        ran.out.contains("watcher-may-act true"),
        "the Group Overrides Global:\n{}",
        ran.out
    );

    let ran = perch(&machine, &["config", "unset", GROUP, "watcher-may-act"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);

    let ran = perch(&machine, &["config", "get", GROUP]);

    assert!(
        ran.out.contains("watcher-may-act false"),
        "and Inherits it again once the Override is cleared:\n{}",
        ran.out
    );
}

/// The four Group arms. Declaring, listing, moving an Account in and taking the
/// Group away again are four different things to reach, and the Group that
/// still holds an Account is the refusal that tells `remove` from `add`.
#[test]
fn the_group_arms_declare_list_move_and_undeclare() {
    let machine = Scratch::holding_an_account("group");

    let ran = perch(&machine, &["group", "add", "spare"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    assert!(ran.out.contains("spare"), "{}", ran.out);

    let ran = perch(&machine, &["group", "move", SOMEONE, "spare"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);

    let ran = perch(&machine, &["group", "list"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    assert!(ran.out.contains("spare"), "{}", ran.out);
    assert!(ran.out.contains(SOMEONE), "{}", ran.out);

    let ran = perch(&machine, &["group", "remove", "spare"]);

    assert_eq!(
        ran.code, EXIT_CONFLICT,
        "a Group holding an Account is not taken away underneath it:\n{}{}",
        ran.out, ran.err
    );

    let ran = perch(&machine, &["group", "move", SOMEONE, "none"]);

    assert_eq!(ran.code, EXIT_OK, "{}{}", ran.out, ran.err);

    let ran = perch(&machine, &["group", "remove", "spare"]);

    assert_eq!(ran.code, EXIT_OK, "{}{}", ran.out, ran.err);
}

/// Both Alias arms, which are one command line apart: `main.rs` reads the
/// absence of a Target as `--unset`, so an arm that read it the other way would
/// free the name it was asked to give.
#[test]
fn the_alias_arms_name_an_account_and_free_the_name_again() {
    let machine = Scratch::holding_an_account("alias");

    let ran = perch(&machine, &["alias", "dev", SOMEONE]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);

    let ran = perch(&machine, &["list"]);

    assert!(
        ran.out.contains("dev"),
        "the Account is named:\n{}",
        ran.out
    );

    let ran = perch(&machine, &["alias", "dev", "--unset"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);

    let ran = perch(&machine, &["list"]);

    assert!(
        !ran.out.contains("dev"),
        "and the name is free again:\n{}",
        ran.out
    );
}

/// The two halves of one command, which are the arms nothing but the reader
/// stops from reaching each other: `Command::Disable` and `Command::Enable`
/// both go to `enable::run`, and swapping the `EnableCommand` they hand it
/// compiles.
#[test]
fn disable_and_enable_reach_their_own_halves() {
    let machine = Scratch::holding_an_account("enable");

    let ran = perch(&machine, &["disable", SOMEONE]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);

    let ran = perch(&machine, &["list"]);

    let row = row_for(&ran.out, SOMEONE);
    assert!(row.contains("disabled"), "{}", ran.out);

    let ran = perch(&machine, &["enable", SOMEONE]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);

    let ran = perch(&machine, &["list"]);

    let row = row_for(&ran.out, SOMEONE);
    assert!(row.contains("enabled"), "{}", ran.out);
}

/// An Account's line in the listing.
fn row_for<'a>(listing: &'a str, email: &str) -> &'a str {
    listing
        .lines()
        .find(|line| line.contains(email))
        .unwrap_or_else(|| panic!("`{email}` is listed:\n{listing}"))
}
