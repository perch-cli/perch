# A fresh provider layout

This prelaunch redesign assumes a fresh installation, explicitly authorized by
the sole user. Historical configuration migrations are removed. Old layouts and
unsupported Exports are refused with reset or restore instructions; Perch does
not reinterpret them as fresh Holdings. This supersedes the historical migration
requirement in ADR the-holdings-outlive-a-perch for the prelaunch reset.

`config.json` owns provider installation settings, global preferences, Scope
policies, Accounts, Aliases, and Group membership in one atomic manifest. Native
Profiles and pending logins live under `providers/<provider>/`; each provider's
`state.json` holds its Default, observations, Quarantines, and Watcher pacing.
A provider is registered in code, not supplied by a configuration entry.

Groups carry stable identities. Renaming a Group changes only the manifest;
provider runtime records continue to name the same identity. A separate journal
for coordinated configuration files is unnecessary. Default Switching still
writes its Landing before changing native Credentials, and holds the provider's
Default lock through its final durable record.

Settings resolve from compiled values through Scope defaults, Scope overrides,
and provider overrides within the Scope. Watcher permission is an explicit
Scope/provider grant; global pause can block those grants but cannot confer one.

Registry layout version 9 and Export version 5 describe this fresh layout.
Exports carry opaque provider Profile bundles, with named Credential and
configuration artifacts. Shared backup workflows do not interpret native file
contents. Each provider validates its artifacts before any restored Profile is
written; a failed manifest commit rolls back the prepared restoration.
