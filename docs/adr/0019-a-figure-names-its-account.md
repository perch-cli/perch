# A figure names its Account

Reading an Account's Utilization means asking Anthropic as that Account, with
the Credential Perch holds for it. Before the figures that come back are cached,
Perch asks the profile endpoint whose access token it just used, and records
nothing when the answer is somebody else.

The Credential this matters for is the live one. Every Account's own Profile
holds a Credential Perch put there, but the active Account is asked about with
whatever is in the Default Profile — and Perch is not the only thing that writes
there. Anyone who runs `claude` and logs into a different Account directly leaves
Perch's record of who is active behind, and a Switch is not required for that to
happen.

Figures cached under the wrong Account would not look wrong. They would look
like that Account having spent quota it never spent, which is the evidence a
Cycle ranks on (ADR 0012) and the evidence a person reads before switching. It
is the one kind of wrong answer this design cannot afford: a plausible one.

A reply that names nobody is not evidence of anything, and does not stop the
read — the same rule liveness already follows, where a Profile is Live because
something says so rather than because nothing does. Nor does a profile endpoint
Perch no longer recognizes: drift in a reply Perch reads for reassurance is no
reason to stop reading Utilization at all.

## Consequences

A refresh spends two requests per Account rather than one. Only the second
counts against the usage allowance ADR 0015 is about, and the first is spent
before it rather than after, so an Account that turns out not to be the one
expected costs nothing from the allowance that matters.

Perch has an early sighting of a live Credential that has changed underneath it.
Nothing acts on that yet — the refusal names the Account the token really
belongs to and stops there — but re-adopting a login that appeared outside Perch
is a thing the tool will eventually want to offer, and this is where it would be
noticed.
