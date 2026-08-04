# Export is encrypted with a required passphrase

`perch export` is the only command that turns keychain-held secrets into a file.
The point of that file is to be kept somewhere durable — a backup drive, a
password manager, another machine — which is precisely where a plaintext bundle
of refresh tokens granting full access to every account someone owns should
never sit. A file like that leaks by being backed up, synced, or committed.

Export therefore encrypts with a passphrase, prompted on export and required on
import. Not optional: an optional passphrase is one people skip, and the failure
is silent until it isn't.

## Considered Options

Plaintext JSON is trivially restorable and inspectable. Rejected because
shipping it honestly would mean telling users to treat the file like an SSH
private key, which most will not.

Offering no export at all is the smallest attack surface, but it makes `perch
purge` and `perch remove` unrecoverable and leaves no way to move accounts
between machines without logging in again everywhere.

## Consequences

A forgotten passphrase means the export is gone, and re-login is the only path
back. That is the correct trade for a file holding every credential at once.
