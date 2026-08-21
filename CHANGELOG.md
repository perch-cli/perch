# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `perch list <group>` and `perch list ungrouped` now say what that Scope has left to draw on — its **Reserve**: how many of the Accounts a Cycle may choose still have Headroom, the best one's own figure, and the age of the reading it came from. Never one pooled figure, since Accounts sit on different plans and Perch only ever sees percentages. A Scope with nothing left says what is in the way rather than only "none", and one whose Accounts a Cycle may not choose says so under the count rather than inside it. A bare `perch list` says no Reserve — it is one table across every Scope with no heading for the sentence to sit under — while every section of `perch list --json` carries a `reserve` at every breadth, as fields, and `null` where nothing has declared the Scope's Accounts interchangeable or where the Scope holds nobody ([#197](https://github.com/perch-cli/perch/issues/197))

### Changed

- [**breaking**] `--json` now answers a script in every field it has. `perch watcher status --json` carried English where a value belonged: `platform` was `a LaunchAgent`, `a systemd user unit` or `a Scheduled Task that runs at logon`, and `log` was a path on some machines, the command `journalctl --user -u perch-watch -f` on others, and the sentence `nowhere — nothing names a log file` where there was none — so nothing could be branched on without matching prose. `platform` is now the arrangement as a word — `launchagent`, `systemd` or `scheduled-task` — and names the arrangement rather than the operating system, because that is what the answer turns on. `log` is now the path or `null`, and the sentence a person reads moves beside it as `log_said`, which is the `reason`/`said` pair a Quarantine already emits. The two camelCase keys Perch had — `binaryExists` here and `upgradeAvailable` in `perch upgrade --check --json` — become `binary_exists` and `upgrade_available`, which is how every other key in every other command is spelled. No exit code moves, and nothing either command prints to a terminal changes

- [**breaking**] every command is now placed by the noun it is about, and the Account is the one left unsaid. The four that broke that rule move onto two nouns: `perch export`, `perch import` and `perch purge` become `perch holdings export|import|purge`, and `perch watch` becomes `perch watcher run` — taking `perch service install|uninstall|status` with it as `perch watcher install|uninstall|status`, because a Service is an arrangement of the Watcher rather than a noun of its own. `perch --help` now lists sixteen names rather than nineteen. `perch watch --once` becomes `perch watcher check`, a verb rather than a flag because it changes both the meaning of the exit code and the lifetime of the command; the exit codes themselves are untouched, and no other flag is affected. `Holdings` — everything Perch holds on this machine — is a glossary term ([#164](https://github.com/perch-cli/perch/issues/164))
- `perch switch` now says what it did rather than why it did it. A Cycle prints where it landed, what that Account was chosen on and the Group it stayed inside, and then the Utilization the Switch bought — three lines where there were seven. Gone are the ranking's defense (`60% headroom, which is true of every one of its Quota Windows — 7-day is its fullest`), the staleness paragraph under the figures, the `Cycling within Group `work`.` opening now folded into the landing line, and the Capture line: a Capture happens before every Switch without exception (ADR 0006), and what happens every time is the guide's to establish once. The five Captures that are *not* the ordinary one still speak, every figure still carries its age (ADR 0015), and the three refusals — everything exhausted, already on the best, nothing declared interchangeable — keep their explanations and their exit codes, because a refusal is the one moment somebody cannot predict what happened ([#223](https://github.com/perch-cli/perch/issues/223))
- the Watcher's decision log now says what a round did and explains itself only where a round refused. `waiting` and `switched` become data — `2026-08-04T12:02:30Z  switched  86% used, fullest 5-hour → overflow@example.com`, down from about forty words — because the threshold is declared once in the opening line and does not change within a run, and `under it` / `over it` is what the status word already means. `cooling`, `nowhere`, `held` and `refused` keep their sentences whole (ADR 0036): nothing happened, and the reader cannot see why. The Account leaves the round line, since the opening names the one being watched and every `switched` line names the one it moved to; the timestamp stays a full RFC 3339 instant, because a `perch watcher check` under cron is read cold days later. `Stopped.` loses the three reassurances that were true of every stop without exception ([#224](https://github.com/perch-cli/perch/issues/224))
- `perch list` now says each invariant fact once beneath the table. A Quarantined Account keeps the reason that is its own — overflow@example.com (as `overflow`): Anthropic would not renew its Credential. — and the `perch relogin` that repairs it closes the block once for the whole Listing rather than ending every line in it, so three broken Accounts no longer mean three copies of the same instruction under a table that already said `quarantined` three times. Where exactly one is broken the repair still names it. The table itself is untouched — columns, alignment, the `*` marker, the Reserve line, every figure's age — as is `perch list --json`. `perch status` answers about one Account (ADR 0053), so it still says the reason and the repair together on its `Quarantine` line ([#225](https://github.com/perch-cli/perch/issues/225))
- the remaining acting commands now say what they did and explain themselves only where they refused. `perch remove` no longer narrates the Credential it deleted, `perch holdings purge` no longer restates what a Purge takes or that Claude Code is still logged in, `perch holdings export` and `perch holdings import` name a count and a path and stop, `perch disable` and `perch enable` say what changed and drop the promise about what a disabled Account still is, `perch relogin` drops the Alias/Group/Cycling column of things a repair left alone, `perch add` no longer repeats after the login what it said before it, `perch group add` and `perch group rename` drop the Settings rows neither of them set, `perch watcher install` and `perch watcher uninstall` drop what starting at login means, and `perch config set` stops defending two of its own designs. Every refusal keeps its explanation and its remedy — a Quarantined Account still says so on both halves of the enable pair, a Remove that destroyed nothing still says why, and a repaired Account that is still disabled is told. Every confirmation prompt is untouched, every figure still carries its age, no exit code moves, and `perch alias`, `perch upgrade`, `perch watcher status`, `perch group list` and `perch config get` were read and needed nothing ([#226](https://github.com/perch-cli/perch/issues/226))

### Removed

- [**breaking**] the Watcher's three departing Settings — `watcher-cooldown-minutes`, `watcher-margin-percent` and `watcher-no-return`. The cooldown and the margin are arithmetic rather than anyone's taste, so they are now constants of 15 minutes and 10 points under the threshold; the no-return could never fire, because the cooldown always reached the decision first. `perch config` carries three Settings, and how full is too full is the only preference left in the loop. A scheduled Check now records only when it Switched, the no-return having been the only thing that read which Account it Switched off ([#161](https://github.com/perch-cli/perch/issues/161))

### Fixed

- a terminal too narrow for the `Status` tab cut a figure's age off, half an email address off a row, and a key hint in two — the tab now breaks between words and between columns instead ([#153](https://github.com/perch-cli/perch/issues/153))
- a registry written by v0.1.0, v0.1.1 or v0.2.0 came back as a serde error about an unknown field. Those three stamped `"version": 1` and the shape moved to 2 without a step between, so every registry any published Perch wrote met this one as unreadable — on a file Perch itself had written. It is now brought forward on the first run that finds it: the active Account, whether Cycling may choose an Account, what each Group declared and what it Inherited from Global, and the Ungrouped Scope that Global became. What a Group inherited is read out of that registry's own Global rather than out of this build's defaults, so a threshold somebody set is the one that arrives, and an Account that was disabled stays disabled. Said once on stderr on the run that rewrites the file, and nothing on stdout moves. The same step runs on the registry inside an Export, so a backup taken before this build imports; an Export or a registry from a *newer* Perch is refused as it always was, and a registry claiming a version no Perch has written — `0`, or none — is now refused rather than read as though it were current, on disk and inside an Export alike. A document stamping one version over another's shape is refused too, rather than read past ([#266](https://github.com/perch-cli/perch/issues/266))

## [0.2.0](https://github.com/perch-cli/perch/compare/v0.1.1...v0.2.0) - 2026-08-14

### Added

- an Upgrade that hands the work back to the Channel that made the Installation ([#133](https://github.com/perch-cli/perch/pull/133))
- dogfood grows to nine phases, and hands the terminal to a person for two of them ([#131](https://github.com/perch-cli/perch/pull/131))
- the wizard refuses a perch the source has moved past, and an Export lands where the command was typed ([#130](https://github.com/perch-cli/perch/pull/130))
- every dogfood run opens with a Repair, and a Quarantine no longer counts as an Account a phase can use ([#127](https://github.com/perch-cli/perch/pull/127))
- dogfood proves what each machine holds, and refuses what it has not been set up for ([#126](https://github.com/perch-cli/perch/pull/126))
- a Group can be renamed, and the rename keeps what the Group carries ([#115](https://github.com/perch-cli/perch/pull/115))
- [**breaking**] Config gains a layer, and the TUI gains the right to change it ([#113](https://github.com/perch-cli/perch/pull/113))
- name the file the PATH line belongs in, and write it where it can be unwritten ([#109](https://github.com/perch-cli/perch/pull/109))

### Fixed

- a Renewal phase that could not tell a 429 from a defect, and read the Credential a Renewal never writes ([#132](https://github.com/perch-cli/perch/pull/132))
- a deep review — an Export sealed at whatever the machine could spare, a lock hiccup with no bound, two fakes that lied to the suite, and sixteen more ([#129](https://github.com/perch-cli/perch/pull/129))
- a deep review — a Dogfood run with no Export behind it, two TUI keys acting from a state nothing drew, and seven more ([#128](https://github.com/perch-cli/perch/pull/128))
- extend test cases, some refactor and fixes ([#122](https://github.com/perch-cli/perch/pull/122))
- a deep review — a watcher that spent an allowance it could not use, a debounced write that undid the next one, and four more ([#120](https://github.com/perch-cli/perch/pull/120))
- a deep review — a marker read as nothing running, a figure that lost its age, and sixteen more ([#116](https://github.com/perch-cli/perch/pull/116))
- read Utilization again — the usage endpoint's `extra_usage` block refused every reply ([#106](https://github.com/perch-cli/perch/pull/106))
- a codebase review — credential loss on a re-run Switch, an Export that would not open, and a leaked lock ([#104](https://github.com/perch-cli/perch/pull/104))
- let the latest-tag check wait for the registry to catch up ([#103](https://github.com/perch-cli/perch/pull/103))

### Other

- dogfood runs prove what each machine holds ([#123](https://github.com/perch-cli/perch/pull/123))
- the site becomes somewhere to read Perch, not just install it ([#121](https://github.com/perch-cli/perch/pull/121))
- *(deps)* bump thiserror from 2.0.19 to 2.0.20 in the cargo-minor-patch group ([#119](https://github.com/perch-cli/perch/pull/119))
- the README becomes a front door, and the prose it was made of becomes a guide ([#118](https://github.com/perch-cli/perch/pull/118))
- release-plz asks the tag rather than a stranger's crate, and the title check leaves the matrix ([#114](https://github.com/perch-cli/perch/pull/114))
- Config gains a layer, and the TUI gains the right to change it ([#111](https://github.com/perch-cli/perch/pull/111))
- the installer writes PATH only where it can be unwritten ([#108](https://github.com/perch-cli/perch/pull/108))

## [0.1.1](https://github.com/perch-cli/perch/compare/v0.1.0...v0.1.1) - 2026-08-10

### Fixed

- put publish = false where release-plz reads it as a decision ([#98](https://github.com/perch-cli/perch/pull/98))
- publish the npm wrapper from a path npm reads as a path ([#97](https://github.com/perch-cli/perch/pull/97))

### Other

- npm has no way to withhold latest, so stop claiming it does ([#101](https://github.com/perch-cli/perch/pull/101))
