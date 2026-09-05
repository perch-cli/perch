# Codex utilization and quota boundaries

Research date: September 5, 2026. Perch baseline: `2b1df7c`.
Context: [Investigate Codex utilization and quota boundaries](https://github.com/perch-cli/perch/issues/430), part of [Chart Codex support for Perch](https://github.com/perch-cli/perch/issues/428).

## Finding

Codex exposes enough information to investigate Utilization display and account selection. Accurate unattended Cycling needs further decisions about metered features, workspace limits, and Credential ownership. Treat this as feasibility evidence, not a validated integration. No live Credentials, quota-consuming requests, logins, or switches are used.

## Subscription is not the quota boundary

**Documented:** Codex access is included in ChatGPT plans. Current pricing explicitly shares ChatGPT Work and Codex usage; local messages and cloud chats share the plan allowance, with weekly limits possible. This does not establish one universal ChatGPT meter. Spark has a separate allowance. Enterprise/Edu flexible pricing uses credits without fixed rate limits; other Enterprise/Edu plans generally follow Plus per-seat limits. API-key usage is billed separately at API rates. [Pricing](https://learn.chatgpt.com/docs/pricing)

**Documented:** Chat and Work are distinct interaction choices. [Use ChatGPT](https://learn.chatgpt.com/docs/use-chatgpt)

**Documented qualification:** Eligible Chat and Work activity can draw from shared workspace credits under credit-based organizational agreements. User limits, workspace overage controls, and the remaining credit allocation differ. Consuming committed credits does not itself create an additional invoice charge. [Work usage and cost](https://learn.chatgpt.com/docs/enterprise/chatgpt-work-usage-and-cost)

**Answer to the user's question:** Yes: normal daily ChatGPT chat and Codex have separate usage allowances, even though the same ChatGPT subscription grants access to both. OpenAI explicitly separates Chat usage and credit rules from Work and Codex, including the listed Pro-model Chat allowances. Work is a different experience and shares Codex usage. [Chat model usage limits](https://help.openai.com/en/articles/20001354-gpt-56-and-gpt-6-pro-in-chatgpt)

The Codex plan article also excludes regular chat from its per-chat credit rule and distinguishes ChatGPT upload, image, and voice limits from Codex. Managed workspace credit arrangements still require the qualification above; a shared billing pool is not evidence that ordinary chat consumes a Codex quota window. [Using Codex with your ChatGPT plan](https://help.openai.com/en/articles/11369540-using-codex-with-your-chatgpt-plan)

## Documented integration surface

Codex app-server provides JSON-RPC `account/rateLimits/read` and `account/rateLimits/updated`. The former includes a compatibility `rateLimits` view and optional `rateLimitsByLimitId`; each bucket has an ID, optional label, primary/secondary windows, usage percentage, duration in minutes, and reset timestamp in Unix seconds. Optional plan, workspace-credit, reached-limit, and earned-reset information also exists. Do not treat an omitted window as 100% remaining. [App-server reference](https://learn.chatgpt.com/docs/app-server)

`account/read` exposes auth mode, email when available, and plan. Its sample does not provide a stable user/workspace composite identity. Managed ChatGPT authentication persists and automatically refreshes tokens. `refreshToken: false` on `account/read` is not a general no-write guarantee. Experimental external-token mode delegates renewal to the host. [App-server reference](https://learn.chatgpt.com/docs/app-server)

`account/usage/read` returns historical token summaries and optional daily totals; it is not a quota percentage. API-key-only authentication is unsupported for that method. Reset redemption and owner-email methods are mutations and are unnecessary for observing utilization. [App-server reference](https://learn.chatgpt.com/docs/app-server)

**Inference:** A documented local protocol is preferable to parsing terminal `/status`, but does not make its underlying HTTP services public APIs. A versioned compatibility probe is still needed.

## First-party implementation evidence

The following observations are pinned to OpenAI Codex commit `ddf04ad26789d040f9ef6a96736f76602e35a6cc`, not presented as stable service contracts:

- The backend client fetches rate-limit status with GET at `/wham/usage` under a ChatGPT backend base, or `/api/codex/usage` for the alternate path style. It distinguishes passive reads from a client opting into Luna reserve behavior. Returned data includes account and user IDs, ordinary-usage allowance, reset credits, and additional limits. [Rate-limit backend implementation](https://github.com/openai/codex/blob/ddf04ad26789d040f9ef6a96736f76602e35a6cc/codex-rs/backend-client/src/client/rate_limit_resets.rs)
- The client normalizes a ChatGPT base to `/backend-api`, adds auth headers and `ChatGPT-Account-Id` when provided, and maps ordinary and additional metered features separately. Its single-result helper prefers `codex`, so using only that helper can hide other limits. Tests cover spend-control exhaustion even without an individual limit. [Backend client](https://github.com/openai/codex/blob/ddf04ad26789d040f9ef6a96736f76602e35a6cc/codex-rs/backend-client/src/client.rs)
- The TUI coalesces periodic reads and invalidates results across account/limit generations, including successful resets. It delays another periodic attempt after completion even on failure. This demonstrates implementation care, not a published polling allowance. [Refresh ordering](https://github.com/openai/codex/blob/ddf04ad26789d040f9ef6a96736f76602e35a6cc/codex-rs/tui/src/app/rate_limit_refresh.rs)

**Unverified:** HTTP endpoint access for Perch, account-ID attribution across organizations, service throttles, response completeness for each plan, credential writes caused by passive app-server use, and concurrent refresh behavior. There is no established free-polling contract or safe numeric interval in the examined docs. Reading status does not require starting an inference turn; that does not prove an unlimited or cost-free service allowance.

## API organization metrics are a different product

The public API exposes organization completions usage with time buckets and filtering/grouping by project, user, API key, model, and service tier; its example uses an admin API key. This is organizational API activity, not a user's Codex subscription allowance. It cannot substitute for Codex Headroom. [Organization usage API](https://developers.openai.com/api/reference/ruby/resources/admin/subresources/organization/subresources/usage/methods/completions)

Workspace spend controls may affect eligible Codex activity depending on the agreement, but do not govern API Platform billing. [Workspace usage controls](https://learn.chatgpt.com/docs/enterprise/usage-limits)

## Comparison with Perch

Perch's [Anthropic adapter](../../src/anthropic.rs) owns unpublished OAuth usage, identity, and renewal endpoints. Its parser requires session and weekly-all readings, uses model-scoped windows, and distinguishes rejection, throttling, drift, and network failure. The repository records roughly 28–30 usage reads per Account per hour; this is Perch's Anthropic evidence and must not become an OpenAI constant. ADR a-window-comes-from-limits.

Perch's [Cycling implementation](../../src/cycle.rs) ranks the worst applicable window. Its [Utilization rendering](../../src/utilization.rs) retains observation age; a failed Refresh preserves old figures, while the Watcher holds when a fresh reading fails. ADR a-figure-carries-its-age.

The existing decision rejects reading Utilization through a client because launching one and renewing Credentials can make observation a write. Codex app-server is a structured integration surface, but its managed renewal creates the same ownership question. Reconsider that decision explicitly before selecting the transport; do not silently route every Watcher round through a client.

## Decisions now possible

1. Prefer the documented app-server surface for a bounded feasibility probe; retain direct HTTP as an explicitly unsupported alternative requiring drift detection and separate auth ownership.
2. Preserve bucket identity, window duration, account/workspace attribution, and optional credit/limit states. Do not translate every bucket into Anthropic's mandatory five-hour/seven-day pair.
3. Rank only limits relevant to the intended Codex workload. Exhausting an unrelated metered feature must not automatically disqualify an Account. Define credit and spend-control behavior before claiming automatic Cycling parity.
4. Keep cached, timestamped display. Determine polling/backoff from observed Codex behavior; never inherit Anthropic's Watcher cadence without evidence.
5. Validate account attribution, read-side Credential changes, exhausted/credit-based plans, and concurrent reads in a later authorized probe. Research alone does not settle these operational facts.

No Registry or Export shape is changed. Any future persistence change follows ADR the-holdings-outlive-a-perch.
