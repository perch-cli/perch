# Codex account lifecycle feasibility

Investigated September 5, 2026 for [Investigate Codex account lifecycle interfaces](https://github.com/perch-cli/perch/issues/429), part of [Chart Codex support for Perch](https://github.com/perch-cli/perch/issues/428). This is research, not an implementation decision. No live Credentials, login, refresh, or switching operations were used.

## Finding

**Codex provides enough documented interfaces to investigate a subscription-backed Perch integration, especially isolated Run. Full Claude behavior is not established.** The largest gap is Switch for already-running clients: current Codex deliberately caches authentication and refuses to reload a different account during guarded refresh. A file replacement must not be presented as a universal live account switch. [Codex auth manager](https://github.com/openai/codex/blob/ddf04ad26789d040f9ef6a96736f76602e35a6cc/codex-rs/login/src/auth/manager.rs#L2025).

Keep two concepts separate: authenticating through a ChatGPT account and measuring the Codex entitlement available to that account. API-key authentication is a separate usage-based route. This report establishes authentication capabilities; the quota investigation determines what each usage counter covers. [OpenAI authentication documentation](https://learn.chatgpt.com/docs/auth).

## Documented integration surface

- Browser login uses `codex login`; device-code login is available, with account/workspace enablement requirements. API-key login accepts a key through stdin. `codex login status` reports the authentication method; `codex logout` clears stored authentication.
- `cli_auth_credentials_store` selects `file`, `keyring`, or `auto`. File storage uses `auth.json` beneath `CODEX_HOME`; automatic mode falls back to a file when the OS store is unavailable. CLI and IDE share cached login. Documentation promises the next start needs login after logout, not immediate switching of every running client.
- Managed policy can force an authentication method or ChatGPT workspace. A mismatching login is rejected. Enterprise access tokens and workload identity also exist; they are additional auth modes, not ordinary subscription login or Platform keys. [Authentication](https://learn.chatgpt.com/docs/auth).

The documented app-server account protocol offers `account/read`, `account/login/start`, cancellation, completion notifications, logout, account updates, and rate-limit reads. Managed ChatGPT mode lets Codex own login, persistence, and refresh; `account/read` with `refreshToken: true` forces refresh. Account reads report ChatGPT email and plan, but email can be absent; API-key accounts do not supply that same identity. External `chatgptAuthTokens` mode requires experimental capability negotiation and makes the host supply fresh tokens on demand. Prefer managed mode for the first investigation; external tokens create a new lifecycle obligation. These methods address the app-server connection being operated, not a broadcast switch API for unrelated processes. [App-server authentication](https://learn.chatgpt.com/docs/app-server#authentication-endpoints).

`CODEX_HOME` scopes local configuration, credentials, history, logs, and caches. A separate home is therefore a plausible Perch Profile, but also isolates more than the Account. Codex `--profile` selects configuration layers; it is not an account-store selector. The current documentation describes separate profile TOML files and explicitly notes a change in Codex 0.134.0, demonstrating why a supported-version boundary matters. [Advanced configuration](https://learn.chatgpt.com/docs/config-file/config-advanced).

## Implementation evidence and fragility

Source inspected at OpenAI Codex commit `ddf04ad26789d040f9ef6a96736f76602e35a6cc`. This is a moving development implementation pinned for reproducibility, not a claim that every released client uses it.

**Credential shape:** `AuthDotJson` contains an auth mode, optional API key, token data, last-refresh time, and additional auth-mode fields. Token data includes access/refresh tokens, an ID token, and optional account ID. ID-token claims distinguish user ID from organization/workspace ID. Perch should not use email alone as a Codex Account identity or assume one login identity means one entitlement. A candidate key is provider plus user and workspace identity; missing fields need an explicit policy. Decoding a JWT supplies claims, not independent identity verification. [Auth storage](https://github.com/openai/codex/blob/ddf04ad26789d040f9ef6a96736f76602e35a6cc/codex-rs/login/src/auth/storage.rs), [token data](https://github.com/openai/codex/blob/ddf04ad26789d040f9ef6a96736f76602e35a6cc/codex-rs/login/src/token_data.rs).

**Store addressing:** the direct keyring backend uses service `Codex Auth` and an account key derived from the canonical home path's SHA-256, truncated to 16 hex characters with a `cli|` prefix. The inspected source also supports a secrets-backed encrypted auth backend; the direct key formula is not the whole storage contract. Moving a home or copying its directory alone can lose access to its Credential. The keyring crate enables native Apple and Windows backends and persistent Linux support. Availability, permissions, fallback, and migration between stores require platform tests. Perch's current macOS-keychain/other-platform-file rule cannot simply be reused. [Storage backends](https://github.com/openai/codex/blob/ddf04ad26789d040f9ef6a96736f76602e35a6cc/codex-rs/login/src/auth/storage.rs#L235), [platform dependencies](https://github.com/openai/codex/blob/ddf04ad26789d040f9ef6a96736f76602e35a6cc/codex-rs/keyring-store/Cargo.toml), [Perch Credential Store](https://github.com/perch-cli/perch/blob/2b1df7c/src/credentials.rs).

**Refresh and concurrency:** the auth manager holds a cached snapshot and a process-local refresh semaphore. Before refreshing it reloads only if the stored account matches; changed same-account tokens can avoid another refresh. The code distinguishes expired, reused, and revoked refresh-token failures. This is evidence against treating copied refresh tokens as independent credentials. The inspected manager does not establish a cross-process refresh transaction that Perch can join. Two homes carrying the same login remain a rotation hazard until tested. [Auth manager](https://github.com/openai/codex/blob/ddf04ad26789d040f9ef6a96736f76602e35a6cc/codex-rs/login/src/auth/manager.rs#L2764).

## Comparison with Perch's lifecycle

The following are feasibility judgments, not completed support. Perch currently places Claude Credentials with read-back verification, captures outgoing Credentials before switching, patches the separate Claude Identity, and records an interrupted Switch in the Registry. Its domain also refuses writes into Live Profiles. These protections remain requirements, but their Claude mechanisms are not Codex contracts. [Perch placement](https://github.com/perch-cli/perch/blob/2b1df7c/src/profile.rs), [Switch](https://github.com/perch-cli/perch/blob/2b1df7c/src/switch.rs), [domain definitions](https://github.com/perch-cli/perch/blob/2b1df7c/CONTEXT.md).

| Perch operation | Feasibility and unresolved condition |
| --- | --- |
| Adoption | Plausible from a recognized Codex cache/store; verify identity and schema, preserve unknown fields, and avoid copying a rotating live login without coordination. |
| Login | Documented CLI/app-server managed flows. Run them against the intended home and retain workspace restrictions. |
| Identity | Structured account reads help display; stable user/workspace discrimination still needs token-shape or protocol evidence across account types. |
| Isolated Run | Strongest first candidate: separate `CODEX_HOME`. Decide which configuration and state may be shared; do not reuse Claude's everything-except rule without an inventory. |
| Switch | Plausible for subsequent launches. Existing CLI, IDE, desktop, and in-flight threads need separate observation; guarded refresh can reject a changed account. |
| Capture | Requires recognizing the freshest same-account Credential and coordinating readers/writers across default and held homes. No documented Capture method was found. |
| Renewal | Delegate to managed Codex refresh where possible. This remains a Credential write, even when invoked through `account/read`. |
| Quarantine / Repair | Perch can retain its policy, but must map terminal refresh errors separately from network/store failures. Account mismatch can mean an external login change, not a dead subscription. Re-login is documented recovery. |
| Remove | Removing a held store is plausible after live-use checks. Logout behavior and deletion of every supported backend need verification; do not equate local removal with server-wide revocation. |
| Export / Import | Preserve opaque Credential content and reconstruct the destination store. Unknown auth formats require refusal. Cross-machine copied refresh tokens must not be used concurrently without a settled ownership model. |

Desktop authentication support is documented, but a CLI/IDE shared-cache statement does not prove every desktop release uses the same home, observes changes at the same time, or shares the same process lifecycle. Codex cloud has a separate browser/service lifecycle and is not controlled by replacing a local cache. API keys also need their own identification, repair, and quota behavior rather than synthetic subscription fields. [Authentication](https://learn.chatgpt.com/docs/auth).

## Smallest future experiments

1. Pin one released Codex version; use synthetic fixtures to verify file and keyring addressing, unknown-field preservation, read-back, rollback, and path migration on macOS, Linux, and Windows.
2. With disposable authorized logins, launch two separate homes. Establish which configuration, plugins, session databases, and caches must remain separate before proposing Shared State links.
3. With two accounts, observe an idle CLI, an in-flight CLI request, an IDE session, and desktop separately before/after a default-store change. Record identity and billing attribution, not merely the UI label. Establish whether restart is required.
4. Exercise same-account concurrent refresh across processes and copied homes, including an interrupted write and a stale restored Export. Identify a coordination mechanism or refuse unsupported concurrency.
5. Exercise personal and workspace logins, missing email, forced-workspace policy, expired/reused/revoked tokens, locked/unavailable stores, and API-key mode. Confirm which failures warrant Quarantine and which warrant retry or instructions.

These experiments are decision prerequisites, not authorization to use the user's live Credentials. No runtime changes, migration, or product behavior is included in this research artifact.
