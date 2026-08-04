# Cycling ranks accounts by their most constrained window

An account has several quota windows at once — five-hour, seven-day, and a
weekly window per model — and they disagree. One account can sit at 10% on its
five-hour window and 95% on its seven-day while another is the reverse, so the
two rank in opposite orders depending on which window you read.

Perch takes each account's worst window and picks the account whose worst is
best. Being blocked by any window blocks you completely, so this is the only
ranking that measures what actually stops you working: when Perch reports 40%
headroom, that is true of every window, and nothing surprising blocks you five
minutes later.

Ranking on the five-hour window alone was rejected. It is the window you hit
first and the simplest to explain, but it will switch you onto an account about
to die on its weekly limit — failing at the exact moment the feature exists to
help, which reads as the tool being broken. A weighted composite of the windows
was rejected as unpredictable and undebuggable.

This fixes how headroom is *measured*, not which account to prefer. Which
account to prefer is a separate, configurable axis — favouring the most headroom,
or the soonest-resetting window so perishable quota is not wasted — and every
such strategy reads headroom the same way.

## Consequences

Cycling skips accounts that are exhausted, disabled, or quarantined. When every
account in the group is exhausted, Perch picks nothing and says so, naming which
account resets soonest, rather than switching somewhere useless.

The ranking gives the picker an honest single number per account, with the
per-window detail beneath it — which is what someone scanning the screen
mid-task needs.
