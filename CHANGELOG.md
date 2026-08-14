# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
