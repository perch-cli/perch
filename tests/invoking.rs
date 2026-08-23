//! Surface: what the built binary accepts, what it prints, and what it exits
//! with (ADR the-binary-proves-its-surface).
//!
//! The one suite that runs `perch` as a process, because `main` cannot be called
//! from a test: which arm a parsed line reaches, and whether the code `ended_as`
//! computed survives to a shell. Behavior stays with the fakes, the parser with
//! `src/main.rs`'s table. The line is operational — **if a case needs a real
//! Claude Code installed, it has crossed** — so `switch`, `add`, `run`,
//! `relogin` and `watcher run` are absent, and a stub `claude` on `PATH` would
//! work around the boundary those arms mark rather than respect it.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use perch::error::{
    EXIT_CONFLICT, EXIT_GENERAL, EXIT_INVALID, EXIT_NOT_FOUND, EXIT_NOT_UNDERSTOOD,
    EXIT_NOTHING_TO_DO, EXIT_OK,
};
use perch::probe::Identity;
use perch::registry::{Account, Registry, Settings};

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
/// `PERCH_HOME` and `CLAUDE_CONFIG_DIR` are ordinary user-facing variables, so
/// isolation here costs no test-only escape hatch and no `#[cfg]`.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    /// A machine Perch has never run on: no home directory at all. Named for
    /// the test that made it, under this process, so two never collide however
    /// many run at once.
    fn untouched(by: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!("perch-invoking-{}-{by}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("claude")).expect("a scratch machine can be made");
        Scratch { root }
    }

    /// The ordinary machine these tests drive: one Account, active and a Cycle
    /// candidate, and one Group declared holding nothing. Written rather than
    /// adopted, because adoption is what asks the machine what Claude Code is
    /// installed — and a registry already there is what stops every command
    /// below asking.
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
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });
        registry.settle(Some(SOMEONE.to_string()));
        registry
            .groups
            .insert(GROUP.to_string(), Settings::default());

        fs::create_dir_all(machine.home()).expect("a scratch home can be made");
        fs::write(
            machine.home().join("registry.json"),
            serde_json::to_string(&registry).expect("a registry is a document"),
        )
        .expect("the registry can be written");
        machine
    }

    /// A machine whose registry an older Perch wrote: the document v0.2.0's own
    /// serde produced, so what the binary meets is a shape a release actually
    /// wrote rather than this tree's memory of one.
    fn holding_what_v0_2_0_wrote(by: &str) -> Scratch {
        let machine = Scratch::untouched(by);
        fs::create_dir_all(machine.home()).expect("a scratch home can be made");
        fs::write(
            machine.home().join("registry.json"),
            include_str!("fixtures/registry-v0.2.0.json"),
        )
        .expect("the registry can be written");
        machine
    }

    fn registry(&self) -> String {
        fs::read_to_string(self.home().join("registry.json")).expect("the registry is there")
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

/// Every command the binary dispatches, as `--help` lists them: the ten that
/// elide the Account, `upgrade`, and the four nouns that are written
/// (ADR a-command-names-its-noun).
const COMMANDS: [&str; 15] = [
    "add", "alias", "config", "disable", "enable", "group", "holdings", "list", "relogin",
    "remove", "run", "status", "switch", "upgrade", "watcher",
];

/// Answered before the parser, and exactly as the Homebrew formula's test block
/// asserts on it. One line and no more: the upgrade notice is for a person at a
/// terminal, and a pipe is not one — which is what keeps a suite that answers no
/// network from asking about a Release.
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

/// Answered before the parser and so outside the path every other command's
/// output goes down: the write failure was thrown away and the process exited 0
/// having written nothing. Linux alone, because `/dev/full` is the one way to
/// fail a write without a closed pipe's timing in it.
#[cfg(target_os = "linux")]
#[test]
fn a_version_that_could_not_be_written_is_a_failure_rather_than_a_silent_zero() {
    let machine = Scratch::holding_an_account("version-full");

    let ran = Command::new(env!("CARGO_BIN_EXE_perch"))
        .arg("--version")
        .env("PERCH_HOME", machine.home())
        .env("CLAUDE_CONFIG_DIR", machine.claude())
        .stdout(std::fs::File::create("/dev/full").expect("/dev/full is there"))
        .output()
        .expect("the binary runs");

    assert_eq!(
        ran.status.code(),
        Some(EXIT_GENERAL),
        "the same code every other command's failed output earns"
    );
    let said = String::from_utf8(ran.stderr).expect("output is UTF-8");
    assert!(
        said.contains("could not write its output"),
        "and it says whose write failed: {said}"
    );
}

/// `--help` lists the commands in the order the enum declares them, and somebody
/// scanning that list is scanning alphabetically. Two had drifted out of it —
/// `run` above `list`, `switch` above `status` — so `list` was to be found
/// between `remove` and `switch`, which is nowhere anyone would look.
#[test]
fn the_commands_are_listed_in_the_order_somebody_scans_them_in() {
    let machine = Scratch::holding_an_account("order");

    let ran = perch(&machine, &["--help"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    // Read off the rendering rather than off `COMMANDS`, which is a copy of the
    // enum kept by hand: what clap prints is what somebody scans.
    let listed: Vec<&str> = ran
        .out
        .lines()
        .skip_while(|line| !line.starts_with("Commands:"))
        .filter_map(|line| line.split_whitespace().next())
        .filter(|word| COMMANDS.contains(word))
        .collect();

    let mut alphabetical = listed.clone();
    alphabetical.sort_unstable();
    assert_eq!(
        listed, alphabetical,
        "the enum declares the order clap renders, and it is the one a reader \
         looks a command up in:\n{}",
        ran.out
    );
    assert_eq!(
        listed.len(),
        COMMANDS.len(),
        "and every command is in it: {listed:?}"
    );
}

/// A command missing from `--help` is one nobody finds.
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

    // The program and that a command is wanted, rather than the whole line:
    // clap names the program from `argv[0]`, so a Windows `perch --help` says
    // `perch.exe`, and which name it was reached by is the shell's business.
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

/// The claim no in-process test can make: the code `ended_as` computed is what a
/// shell reads. Every line is a real dispatch arm over real bytes, and between
/// them they carry the codes the reachable commands answer with.
#[test]
fn the_code_a_command_ended_as_is_what_the_process_exits_with() {
    let machine = Scratch::holding_an_account("codes");

    for (line, code) in [
        (&["list"][..], EXIT_OK),
        (&["disable", "nobody@example.com"], EXIT_NOT_FOUND),
        (&["group", "move", SOMEONE, "nowhere"], EXIT_NOT_FOUND),
        (&["group", "add", GROUP], EXIT_CONFLICT),
        (&["alias", SOMEONE, GROUP], EXIT_CONFLICT),
        (
            &["config", "set", GROUP, "watcher-threshold-percent", "500"],
            EXIT_INVALID,
        ),
        (
            &["config", "set", "nothing-of-the-sort", "true"],
            EXIT_NOT_FOUND,
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

/// The one place a reachable command answers "there was nothing to do".
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

/// Asserted at the process, where the two streams are genuinely separate file
/// descriptors rather than one buffer a test handed in.
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

/// Only that a refused line ends the process at 2 with nothing on standard
/// output. Which of these clap refuses, and in what words, is asserted against
/// the table in `main.rs`.
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
        // A check asks what the *newest* Release is and installs nothing, so
        // there is nothing for either of these to say to it — and ignoring them
        // would answer about the newest without mentioning it.
        &["upgrade", "--check", "--release", "0.1.0"],
        &["upgrade", "--check", "--yes"],
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

/// The order those two run in, which only a process shows: this line is read off
/// the words before the parser sees them, and clap would otherwise refuse it in
/// clap's own words.
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

/// A positional Scope and two flags, where a Scope arriving where `--json` was
/// read would answer a script about everything. `--refresh` is the one flag this
/// suite cannot press, being the only thing in Perch that touches the network, so
/// what is claimed is that nothing else on the arm is it.
#[test]
fn the_listing_arm_takes_a_scope_and_renders_a_table_or_a_document() {
    let machine = Scratch::holding_an_account("list");

    let ran = perch(&machine, &["list"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    assert!(ran.out.contains(SOMEONE), "{}", ran.out);
    assert!(ran.out.contains("Utilization"), "{}", ran.out);
    assert!(
        !ran.out.contains("In no Group"),
        "a listing of everything is under no heading:\n{}",
        ran.out
    );

    let ran = perch(&machine, &["list", "ungrouped"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    assert!(
        ran.out.contains("In no Group"),
        "and a Scope narrows it to the one named:\n{}",
        ran.out
    );

    let ran = perch(&machine, &["list", "--json"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    let document: serde_json::Value =
        serde_json::from_str(&ran.out).unwrap_or_else(|_| panic!("a document:\n{}", ran.out));
    assert_eq!(document["active_account"], SOMEONE, "{}", ran.out);
    assert_eq!(document["scope"]["kind"], "all", "{}", ran.out);
}

/// The status arm, which cannot be widened to a set
/// (ADR the-listing-owns-the-set): a Scope typed here is a line the parser
/// refuses rather than one that quietly lists a Group.
#[test]
fn the_status_arm_answers_about_one_account_and_takes_no_scope() {
    let machine = Scratch::holding_an_account("status");

    let ran = perch(&machine, &["status"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    assert!(ran.out.contains(SOMEONE), "{}", ran.out);
    assert!(
        !ran.out.contains("In no Group"),
        "the question is about one Account:\n{}",
        ran.out
    );

    for widened in [&["status", "--group"][..], &["status", "ungrouped"]] {
        let ran = perch(&machine, widened);

        assert_eq!(
            ran.code,
            EXIT_NOT_UNDERSTOOD,
            "`perch {}` is the listing's line rather than this one:\n{}{}",
            widened.join(" "),
            ran.out,
            ran.err
        );
    }

    let ran = perch(&machine, &["status", "--json"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    let document: serde_json::Value =
        serde_json::from_str(&ran.out).unwrap_or_else(|_| panic!("a document:\n{}", ran.out));
    assert_eq!(document["active"]["email"], SOMEONE, "{}", ran.out);
}

/// Told apart by what they do to a Scope: a `set` that reached `get` would change
/// nothing, and a `get` that reached `set` would report a value nobody asked for.
#[test]
fn the_config_arms_read_a_setting_and_change_one() {
    let machine = Scratch::holding_an_account("config");

    let ran = perch(&machine, &["config", "get"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    assert!(
        ran.out
            .contains(&format!("{GROUP} watcher-threshold-percent 80")),
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
        ran.out.contains(&format!("{GROUP} watcher-may-act true")),
        "the Group holds what was said about it:\n{}",
        ran.out
    );
}

/// Four arms, where the Group that still holds an Account is the refusal that
/// tells `remove` from `add`.
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

/// One command line apart: `main.rs` reads the absence of a name as `--unset`, so
/// an arm reading it the other way would free the name it was asked to give.
#[test]
fn the_alias_arms_name_an_account_and_free_the_name_again() {
    let machine = Scratch::holding_an_account("alias");

    let ran = perch(&machine, &["alias", SOMEONE, "dev"]);

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

/// The two arms nothing but the reader stops from reaching each other: both go to
/// `enable::run`, and swapping the `EnableCommand` they hand it compiles.
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
    assert!(
        !row.contains("disabled"),
        "the state cell empties rather than naming a state with no name:\n{}",
        ran.out
    );
}

/// An Account's line in the listing.
fn row_for<'a>(listing: &'a str, email: &str) -> &'a str {
    listing
        .lines()
        .find(|line| line.contains(email))
        .unwrap_or_else(|| panic!("`{email}` is listed:\n{listing}"))
}

/// End to end, through the process: the wound this repairs was that every
/// registry any published Perch wrote came back as serde's words about an
/// unknown field. A listing is the cheapest command that proves one is read, and
/// the file left behind is the proof the step is paid once
/// (ADR a-registry-comes-forward).
#[test]
fn a_registry_a_published_perch_wrote_is_read_and_written_forward() {
    let machine = Scratch::holding_what_v0_2_0_wrote("older-registry");

    let ran = perch(&machine, &["list"]);

    assert_eq!(ran.code, EXIT_OK, "{}", ran.err);
    assert!(ran.out.contains("work@example.com"), "{}", ran.out);
    assert!(
        ran.out.contains("disabled"),
        "the Account it had disabled still is: {}",
        ran.out
    );
    assert!(
        ran.err.contains("brought forward"),
        "and the rewrite is said on stderr rather than in the listing: {:?}",
        ran.err
    );

    let written = machine.registry();
    assert!(written.contains("\"version\": 2"), "{written}");
    assert!(!written.contains("\"global\""), "{written}");

    let again = perch(&machine, &["list"]);
    assert_eq!(again.code, EXIT_OK, "{}", again.err);
    assert!(
        again.err.is_empty(),
        "and the next run has nothing to say: {:?}",
        again.err
    );
}
