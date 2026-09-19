# Provider contracts and configuration

Status: accepted design, implementation in progress. Shared Add/Relogin/Run,
Default recovery and Switching, opaque backup bundles, provider quota ranking,
independent Watcher scheduling, structured credential removal, and the
manifest/runtime split are implemented. Liveness corroborates normalized
session evidence supplied by each provider. Probe and Triage consume structured
provider diagnostics and native session preparation. Service installation uses
provider-declared executable candidates, probe arguments, and nonsecret
environment, with shared service-manager rehearsal and rendering. Default
inspection keeps its native guard opaque and rejects foreign-provider Profiles
before reading their Credentials. Claude Default resolution and fallback identity
serialization are private; shared Profile and pending-login paths require an
explicit provider. Shared Accounts no longer expose a native Credential Store.
Claude credential storage, native format recognition, and native lock assumptions
are private. There are no root aliases to those modules. Test fixtures arrange
synthetic native state through Host independently of the implementation. Claude
login and discovery enroll stable Account identities; Relogin updates email
without changing storage. Fixtures use stable references for enrolled Accounts.
Run opens a configured provider and carries its resolved installation through
launch preparation. Client and custom-command requests are distinct; adapters
neither reinterpret command words nor resolve the selected executable again.
Add and Relogin authenticate through the selected installation; Claude checks
that same executable for its version before login. Service setup shares
configuration resolution. Discovery uses the selected installation. Usage opens
one configured handle per observation; native adapters decide when they need an
installation. Claude defers its pinned executable's version read until a refusal
needs it. Diagnostics use one resolved installation and one version result;
interactive diagnostic sessions carry that installation through execution.
Codex app-server exchanges check cancellation and renew exclusive access while
child reads or writes are pending, with a bounded deadline and child cleanup.
Profile bundles enforce 16 MiB of content, 256 artifacts, and 1024 bytes per
artifact name before encryption and restore preparation. Validation follows
whole-Export decryption and parsing; it is not an overall import memory bound.
A test-only third provider exercises catalog selection, Add, Run, Group-scoped
usage refresh, configuration persistence, Export, and Watcher capability refusal.
Run supplies shared Profile state only when the provider declares support; the
facade rejects unsupported sharing requests before native effects.
The third-provider restore checks cover validation before all writes, rollback
of earlier and partially written Profiles, metadata failure, successful commit,
and preservation of repaired Credentials after metadata failure. Native Default
inspection confirms that the journal precedes live writes and the provider guard
covers the final Registry save. Restore and installation handles expose explicit
rollback results. Add, adoption, and Import attempt every owned cleanup and
report incomplete recovery while preserving the original failure. Destructors
remain a fallback; ordinary failure paths no longer rely on silent cleanup. The shared identity
contract permits an absent Workspace; Codex requires one. Claude local Default
matching, capture, backup, and usage fallback recognize stable subjects. A live
Credential without readable identity is not captured into a stable subject unless
it already equals that Account's saved copy, in which case nothing is copied.
Backup falls back to the saved copy when live identity cannot be established.
Claude remote usage attribution compares the profile response's account and
organization UUIDs to the expected identity. Missing or malformed IDs refuse the
observation even when the email matches; native metadata cannot override that
refusal. Registry layout 9 and Export version 5 carry the identity shape.
The contract below is the target; it is not a claim that every seam is complete.

## Problems this design addresses

- `providers/provider.rs` accepts `commands::add::AddArgs`, the whole Registry,
  terminal writers, and Claude's `probe::Installed`. It delegates commands rather
  than isolating tool mechanics.
- `providers/mod.rs` publishes the concrete adapters and the Anthropic client.
  `lib.rs` re-exports implementation modules, giving callers another route around
  the interface.
- Claude's `add`, `run`, and `relogin` implementations own workflow decisions
  that Codex implements independently. Different file layouts obscure that
  duplication; giving them matching filenames would not remove it.
- Shared `probe`, `credentials`, `profile`, `login`, `carry`, `reconcile`, and
  `switch` code contains Claude knowledge. Moving only the HTTP client leaves
  most of the integration outside the provider module.
- Shared Cycling understands Fable-specific windows and preferences. The
  provider seam therefore needs normalized quota semantics as well as login
  and process operations.
- Splitting Accounts, Groups, provider preferences, and runtime state into
  independently written documents introduces a multi-file metadata transaction.
  The current `metadata.pending.json` implementation is a consequence of that
  split, not a requirement of supporting several providers.

## Module ownership

| Module | Owns | Does not own |
| --- | --- | --- |
| Commands | Argument parsing, target resolution, workflow order, reporting | Native filenames, authentication protocols, provider branches |
| Shared domain | Accounts, Aliases, Groups, identity references, normalized quota, Cycling policy | OAuth payloads, model-specific window names, processes |
| Configuration | Parsing, validation, setting resolution and provenance | Native client configuration formats |
| Holdings repository | Perch paths, its manifest, operation records, atomic saves | Interpreting a native Credential |
| Provider interface | Catalog, configured provider handles, structured requests/results, operation capabilities | Clap arguments, whole command workflows |
| Private adapters | Authentication, native Profiles, client discovery, launch preparation, usage normalization, native Default changes | Alias selection, Group membership, Perch output prose, saving the manifest |
| Host | Files, private permissions, atomic replacement, processes, HTTP, platform keychain transport, time | Whether a file contains Codex auth or Claude settings |

There is one implementation of the Relogin workflow. It resolves an Account,
authenticates through its Provider, verifies the returned Identity, rechecks the
Holdings after the browser interaction, installs the replacement through the
Provider, records the outcome, and reports it. Add reuses authentication; it
differs in shared policy about duplicates, Alias ownership, and Group placement.
Neither adapter implements `relogin(AddArgs, Registry, stdout)` or an equivalent
command-shaped method.

Common file I/O does not imply common file meaning. Atomic replacement belongs
to Host. Knowing which fields of `.claude.json` may be carried, or which Codex
configuration forces a file Credential Store, belongs to the adapter.

## Rust visibility and layout

```text
src/
  commands/                 one workflow per command
  domain/                   provider-neutral values and policy
  config/                   settings schema and resolver
  holdings/                 Perch persistence and operation recovery
  host/                     effects and platform implementations
  providers/
    mod.rs                  publishes only provider
    provider.rs             catalog and public contract
    claude/
      mod.rs                implements the private adapter contract
      auth.rs               login and Credential handling
      profiles.rs           native state, carry, and Default mechanics
      usage.rs              Claude service requests and normalization
      process.rs            CLI discovery and launch preparation
    codex/
      mod.rs                implements the same private adapter contract
      auth.rs
      profiles.rs
      usage.rs
      process.rs
```

The adapter modules are private: `mod claude; mod codex;`. Only `pub mod
provider;` is exported. Callers import `crate::providers::provider::{...}`.
There are no root aliases exposing `claude`, `codex`, `anthropic`, or native
types. Anthropic HTTP code is part of Claude's private usage/auth implementation;
there is no second provider or public `anthropic.rs` module.

Both tools are organized by native responsibility, not by CLI command. Private
helpers may differ when their tools differ; matching public contracts do not
require empty helper files or one enormous source file per tool.

Use Rust visibility to prevent external access to adapters, and architecture
tests to prohibit reverse dependencies from adapters into commands, reporting,
or the Holdings repository. Shared domain values and Host are allowed
dependencies. Provider conformance tests cross the public interface; native
protocol unit tests can live inside their private modules.

A separate Cargo workspace is not necessary for this redesign. If stronger
crate-level dependency enforcement becomes useful, the same seam can be moved
into a provider crate without changing command workflows.

## Structured contract

The public catalog opens a configured Provider handle. A private adapter trait
implements its native operations. The public handle applies common capability
and access checks, so commands do not repeat those checks for every tool.

The contract is organized around these operations, not command names:

| Operation | Structured input | Structured result |
| --- | --- | --- |
| Discover | Resolved installation settings | Installation and adoptable Identity/Profile evidence |
| Authenticate | Login intent, staging location, optional expected Identity | Authenticated staged Profile and Identity |
| Inspect Profile | Profile reference and access context | Identity, usability, and liveness evidence |
| Install Profile | Staged Profile, destination, expected Identity, exclusive access | Applied change with an explicit commit/recovery outcome |
| Prepare launch | Profile reference, launch kind, arguments, access context | Prepared launch holding its resources until the process exits |
| Observe usage | Profile reference, resolved workload preference, operation control | Attributed normalized quota, completeness, and retry information |
| Change native Default | Expected current Identity, destination, operation control | Applied, unchanged, unsupported, or recovery-required outcome |
| Snapshot / restore | Profile reference or opaque Profile bundle | Secret-safe bundle or prepared change |
| Forget Profile | Profile reference and exclusive access | Removal outcome, including external Credential Stores |

These are semantic contracts; their final Rust grouping can use subordinate
handles rather than a large trait with a method for every command. Authentication
and prepared changes are owned handles: callers cannot fabricate them from a
path or a boolean and skip the checks that made them valid.

Requests contain only what the operation needs:

- `ProviderId` is validated through the catalog. Persistence and the generic
  `--provider` option do not need a provider enum match in every consumer.
- `AccountIdentity` contains a stable provider subject and an optional Workspace
  identity, plus a separate display description. Email is not the storage key;
  not every future provider must invent a Workspace.
- `ProfileRef` carries an opaque Account ID, its Provider, and a repository-issued
  location. It contains neither an Alias nor Group settings.
- `LaunchRequest` distinguishes a coding-client launch from a custom executable.
  Arguments are arrays and environment changes are structured; no shell command
  string is assembled. A custom command does not require the coding CLI.
- `ResolvedPolicy` contains concrete values. Adapters do not interpret global or
  Group inheritance. Native options are validated into a private typed value at
  the facade; commands never inspect arbitrary provider JSON.
- `OperationControl` carries cancellation, a deadline, and access renewal. A
  provider must honor it inside long calls, not only between Accounts.
- Results distinguish missing authentication, Identity mismatch, a busy Profile,
  throttling, unsupported behavior, and incomplete protocol data. They carry
  safe details; Perch's command layer renders the final message.

The provider does not receive an entire Registry, Export, `AddArgs`, or stdout
writer. Native interactive login can inherit the terminal through Host. Any
Perch progress event is structured and rendered by the caller.

Credentials remain secret-safe opaque values or owned staged Profiles. Token
decoding, keychain service names, environment filtering, and native configuration
parsing are private. Generic snapshot code does not assume a Profile is exactly
one Credential string and one `.claude.json`; a bundle can carry several native
files and external-store payloads, with validated relative paths and size limits.

## Workflows and failure semantics

Shared workflows own when the Registry is locked and when it must be reread.
Browser login and long-running client processes hold no Registry lock. The
provider owns native access coordination and retains any native leases a
prepared operation needs. Neither side can claim that a Perch Marker alone
proves an external client is idle.

The order for Relogin is resolve, authenticate into staging, reacquire/reload,
verify that the same Account still exists, install, persist the resulting
metadata, report. A different Workspace is a different Identity and cannot
replace the selected Account. An Alias or Group changed during login is retained
from the reread, not overwritten by the initial copy.

Applied changes are not all reversible. In particular, a successful Credential
renewal may invalidate the old secret. A generic Drop implementation must not
blindly restore that secret. The contract distinguishes a reversible staged
change from an applied change requiring recovery; shared workflows preserve
and report the provider's outcome. Default Switching retains a durable operation
record before touching native stores. Removing the multi-document metadata
journal does not remove Credential recovery or lock requirements.

## Quota, Switching, and the Watcher

Providers return attributed quota observations with validated percentages,
reset times, applicable constraints, and explicit completeness. Missing or
unrecognized applicable limits are unknown capacity, never 100% Headroom.

The adapter maps native model/limit names to normalized capacity for the selected
workload. Shared policy can rank complete capacity by Headroom or reset time
without naming Fable or parsing Codex limit IDs. An optional preference tier is
normalized by the provider; the choice between Accounts stays in shared Cycling.

Every Cycle is restricted to `(Provider, Scope)`. A mixed Group is not a pool in
which Claude quota can substitute for Codex quota. Provider Default, cooldown,
retry scheduling, and burst history are independent. One Watcher process can
schedule several providers, maintaining separate pacing and next-due times;
one provider's throttle cannot delay another provider's due round.

Capabilities distinguish native Default changes from changes that an already
running client can adopt. A generic interface cannot turn an unsupported Codex
live change into a supported one. Existing isolated Runs remain pinned. A named
Account identifies its Provider; an ambiguous mixed-Group Switch requires an
explicit Provider unless reliable invocation context identifies it.

Adding a provider means implementing an adapter, registering it once, and passing
the contract tests. The generic `--provider <id>` path, workflows, scheduler,
and repository do not gain a new provider-specific branch. Existing `--claude`
and `--codex` remain convenience selectors.

## Configuration recommendation

Use one authoritative Perch manifest, `config.json`, containing global settings,
registered-provider configuration, Group declarations/settings, Ungrouped
settings, and the non-secret Account directory. The Registry remains a domain
concept and a typed view of that directory; it need not be a second JSON file.
An Account's Alias and Group are each recorded once, in its directory entry.

```text
$PERCH_HOME/
  config.json
  operations/               durable records for incomplete Holdings operations
  locks/
  providers/
    claude/
      state.json            Default, Account health, cached quota, watcher state
      profiles/<account-id>/
      pending/<operation-id>/
    codex/
      state.json
      profiles/<account-id>/
      pending/<operation-id>/
```

There is no `providers.json` beside another file containing provider preferences,
and no root `registry.json` carrying a second copy of Group names. Keeping all
user-owned names and membership together makes a Group rename one atomic write.
The cost is a larger configuration document containing Account descriptions as
well as preferences. That is preferable here to a second namespace joined across
files or a new metadata transaction subsystem.

Per-provider runtime documents are owned by shared persistence, not by adapters.
Native files inside Profiles are adapter-owned. Runtime references use stable
Account IDs, so Alias or email changes do not relocate Profiles or change Default
references. Removed-account runtime entries can be pruned; they never recreate an
Account absent from the manifest. An interrupted Default change is reconciled
through its operation record, not silently discarded as cache.

Native client default homes and platform keychains may remain outside
`$PERCH_HOME`; their adapters own those external locations. The directory tree
does not imply that every native Credential becomes a file inside Perch.

Use a distinct format identifier and a version for the fresh manifest. Reject
old layouts with instructions; do not restore the removed migration machinery.
Exports also carry their own format/version and the writing Perch version.
Config or credential-format changes after this deliberate reset need their own
compatibility decision and cannot silently reuse a published version.

## Setting ownership and resolution

Keep application settings separate from defaults for Scope policy:

| Location | Settings |
| --- | --- |
| `global.run` | Default Provider, whether an implicit selection may fall back to another installed enabled Provider |
| `global.watcher` | A pause switch that stops automatic changes everywhere without erasing local grants |
| `providers.<id>` | Enabled state, CLI path, adapter-validated native options |
| `scope_defaults` | Strategy and numeric watcher thresholds/margins |
| `groups.<name>` | Scope policy overrides and per-provider watcher grants/options |
| `ungrouped` | The same policy, plus the explicit interchangeability declaration |

For ordinary policy values, resolve compiled defaults, then `scope_defaults`,
then the named Scope, then that Scope's Provider override. Return a typed
`ResolvedPolicy` with the source of every value. This introduces a small,
explicit inheritance rule; it revisits ADR a-setting-names-its-scope rather than
claiming the existing no-inheritance decision already allows it.

Permission does not use that cascade. Watcher grants are explicit for each
`(Scope, Provider)` and default to false. A global pause, disabled Provider,
unsupported operation, or ungranted Ungrouped interchangeability blocks action.
Removing a block restores an existing grant; it never creates one. New providers
and new Groups therefore do not acquire unattended Switching permission from a
global default. This preserves the grant principle in ADR a-setting-names-its-scope.

Do not add provider-wide thresholds as a fifth policy layer. Provider-wide
configuration describes the installed tool, while cycling policy describes
which Accounts may substitute for each other. Native model preference belongs
under the Scope's provider options, not as `prefer_fable` in common Settings.

Useful additions are `run.fallback`, the global watcher pause, and a read that
shows effective settings with their sources. Retain the existing strategy,
threshold, and margin. Do not expose arbitrary polling intervals, OAuth URLs,
auth-store paths, or retry limits as casual settings; protocol budgets and native
safety requirements stay adapter-owned until there is a concrete need to tune
them. CLI paths must be validated without requiring login; disabled providers
remain available for backup, removal, and configuration repair.

The accompanying `provider-config.example.json` is an illustrative new manifest,
not a file the unfinished implementation can consume. In that example, Claude's
work watcher uses the default 80% threshold; Codex's work watcher is explicitly
disabled and its 75% override applies only if permission and capability later
allow it. No cross-provider quota comparison is implied.

## Implementation gates

The implemented workflows are checked with both native adapters and an injected
third provider. Restore tests cover validation, partial writes, metadata failure,
commit, and failed cleanup; repair tests verify that fresh Credentials survive a
metadata failure. The gates below record the implementation sequence.

1. Replace the command-shaped trait and remove public implementation re-exports.
   Make the neutral Identity, Profile, usage, and operation outcomes concrete.
2. Move native code behind the two private adapters. Rebuild Add and Relogin
   around one authentication/install workflow; Run around one prepared-launch
   workflow; Export/Import around one opaque-bundle workflow.
3. Replace the provisional multi-document configuration with the single manifest
   and explicit resolver. Keep native Credential recovery independent.
4. Remove native model rules from shared quota policy. Run every Cycle and
   Watcher schedule within its Provider and Scope, with independent pacing.
5. Validate the architecture through visibility checks and a fake third adapter
   accepted by an injected catalog, without editing commands or the scheduler.

Behavior checks must cover same-email/different-Workspace identities, conflicting
selectors, disabled/absent/custom-path providers, custom executable Runs, login
interruption, concurrent Alias/Group edits during login, incomplete quota,
independent watcher timing, loss of access during native changes, and rollback
versus recovery after metadata failure. Backup/restore must cover arbitrary
native bundles and keychain-backed Profiles without leaking secrets.

The existing suite remains a source of behavioral requirements, not a reason to
retain the old architecture. Tests for deliberately removed layouts are replaced
with fresh-layout refusals; tests for Credentials, concurrency, and recovery must
keep exercising those guarantees through the new public contract.

## Claude profile identity evidence

The installed official `@anthropic-ai/claude-code` package, version 2.1.270,
was inspected on September 14, 2026 without running a login or reading credentials.
Its `/api/oauth/profile` reader validates `account.uuid`, `account.email`, and
`organization.uuid` as strings. The caller maps those fields to `accountUuid`,
`emailAddress`, and `organizationUuid` in its native identity. Perch uses those
UUIDs for token attribution and rejects empty IDs. This is client implementation
evidence, not a public stability guarantee for the endpoint. Synthetic behavior
tests cover matching identities after an email rename, mismatched users and
Workspaces sharing an email, and incomplete or malformed profile replies.
