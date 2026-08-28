# A figure names its Account

Reading an Account's Utilization means asking Anthropic as that Account, with the
Credential Perch holds for it. Before the figures that come back are cached,
Perch asks the profile endpoint whose access token it just used, and records
nothing when the answer is somebody else.

The Credential this matters for is the live one. Every Account's own Profile
holds a Credential Perch put there, but the active Account is asked about with
whatever is in the Default Profile — and Perch is not the only thing that writes
there. Anyone who runs `claude` and logs into a different Account directly leaves
Perch's record of who is active behind, and a Switch is not required for that to
happen.

Figures cached under the wrong Account would not look wrong. They would look like
that Account having spent quota it never spent, which is the evidence a Cycle
ranks on (ADR headroom-is-the-worst-window) and the evidence a person reads
before switching. It is the one kind of wrong answer this design cannot afford: a
plausible one.

## Drift moves the question to this machine rather than ending it

A profile endpoint Perch no longer recognizes — a body that will not parse, a
renamed field, an address that has stopped being sent — is drift. Drift is no
reason to stop reading Utilization, and it is no evidence about the Credential
either way. What it is a reason to stop doing is taking Anthropic's word for
whose the Credential is, because there is no longer a word to take.

**Letting the read simply go on was too wide a carve-out.** The day the profile
endpoint stopped naming an address, this check became a no-op for every Account
on every machine, for ever, with a remark printed above the figures and nothing
else: exit 0, no finding in a probe, nothing in the Trail. That is strictly worse
than the outage this endpoint's other failure mode causes, because an outage ends
and a rename does not — and the exposure is a machine holding several Accounts,
in a tool whose reason to exist is keeping them apart.

So drift falls back to the evidence already on the machine, which is the evidence
a Quarantine has always been held to. An Account's own Profile is a directory
only Perch writes into, so a Credential found there is that Account's. The
Default Profile is not, and beside the Credential it carries the Identity saying
whose Claude Code last logged in as — which is the case this check exists for,
since anyone who runs `claude` and logs into a different Account leaves Perch's
record of who is active behind. Where that Identity names somebody else, or
cannot be read at all, no figure is recorded and the attempt says why.

The remark stays and names what the check is resting on instead. A rename nothing
remarked on would be indistinguishable from Anthropic still answering.

## What does not stop the read

A reply that names nobody is not evidence of anything, and does not by itself
make a Credential somebody else's — the same rule liveness already follows, where
a Profile is Live because something says so rather than because nothing does.
The read stops on what this machine says, never on Anthropic's silence.

An HTTP failure is not folded in with drift either. A 500, a 502 or a 404 at a
URL that used to work says nothing about the Credential and nothing about the
Account, and reading one as drift turns an outage into permission to fall back to
a weaker answer.

## Consequences

A refresh spends two requests per Account rather than one. Only the second counts
against the usage allowance ADR a-figure-carries-its-age is about, and the first
is spent before it rather than after, so an Account that turns out not to be the
one expected costs nothing from the allowance that matters.

While the profile endpoint is drifting, an Account read out of the Default
Profile needs a readable Identity naming it. A machine whose `.claude.json` will
not parse loses the live Account's fresh figures until the endpoint is understood
again — the cached figure still shows and the command still exits 0, which is the
degradation a Refresh already has for everything it could not read. Accounts read
out of their own Profiles are untouched.

Perch has an early sighting of a live Credential that has changed underneath it.
Nothing acts on that yet — the refusal names the Account the token really belongs
to and stops there — but re-adopting a login that appeared outside Perch is a
thing the tool will eventually want to offer, and this is where it would be
noticed.
