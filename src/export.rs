//! Everything Perch holds, as one `age` file (ADR the-holdings-go-out-sealed).
//!
//! Two halves, and they are separate on purpose. **Gathering** reads the
//! Registry and every Credential out of the stores they live in, and is the
//! only part of an Export that touches the machine. **Sealing** turns what was
//! gathered into the bytes that go in the file, and is arithmetic: given an
//! Export and a passphrase it is the same answer on every machine, so it is
//! testable without one.

use std::collections::BTreeMap;

use age::secrecy::SecretString;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::name;
use crate::registry::{Account, Registry};

/// The envelope version; the Registry inside carries its own layout version.
pub const CURRENT_VERSION: u32 = 5;

/// The most scrypt work [`unseal`] will spend opening one file, as `log2(N)`.
///
/// Fixed rather than measured, so whether an Export opens is not a question about
/// the pair of machines it traveled between. One above what Perch writes: the
/// factor sizes a buffer before the passphrase can be doubted.
const MAX_WORK_FACTOR: u8 = 20;

/// The scrypt work [`seal`] spends writing one file, as `log2(N)`.
///
/// Fixed for [`MAX_WORK_FACTOR`]'s reason and for one worse: `age`'s own
/// calibration has no floor, so a CPU-starved machine seals at 2^10 in silence.
/// 19 is above `age`'s second-of-work guess of 18, and is paid twice per file.
const WORK_FACTOR: u8 = 19;

/// What Perch spends sealing has to stay under what it will spend opening, or
/// every Export it writes is one it refuses to read. At compile time, because
/// there is no run in which it is worth discovering.
const _: () = assert!(WORK_FACTOR < MAX_WORK_FACTOR);

/// The oldest envelope shape any Perch has stamped; below it names no shape.
///
/// The Export's own rather than the Registry's, which it equals by coincidence:
/// the day that floor moves, every Export ever written claims a version the
/// Registry's number says nothing wrote.
const EARLIEST_VERSION: u32 = CURRENT_VERSION;

const _: () = assert!(EARLIEST_VERSION <= CURRENT_VERSION);

/// An Export, unsealed: what one `age` file holds before it is encrypted and
/// after it is decrypted again.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub version: u32,
    /// The whole Registry — every Account, its Alias, its Group, whether
    /// Cycling may choose it, why it is Quarantined, and what each Group
    /// carries. Written whole rather than field by field, so a Setting added to
    /// a Group is in the next Export without anybody putting it there.
    pub registry: Registry,
    #[serde(deserialize_with = "crate::json::unique_map")]
    pub profiles: BTreeMap<String, crate::providers::provider::ProfileBundle>,
}

impl std::fmt::Debug for Export {
    /// Counts and addresses, never secrets: which Accounts an Export holds a
    /// Credential for is the question somebody debugging one has, and it is
    /// answerable without rendering a token. By hand for the reason
    /// native Credentials are — a derived one prints every field, and a
    /// formatting specifier is all that stands between these values and a log.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut said = formatter.debug_struct("Export");
        said.field("version", &self.version)
            .field("registry", &self.registry);
        for (what, held) in self.payloads() {
            said.field(what, &held.keys());
        }
        said.finish()
    }
}

/// Reads everything Perch holds: the Registry it was handed, and the Credential
/// in each Account's Credential Store.
///
/// Nothing is Renewed and nothing is Rotated. A store that will not say what it
/// holds stops the Export rather than being recorded as an Account with none.
pub fn gather(host: &dyn Host, registry: &Registry) -> Result<Export> {
    // Filled in place rather than gathered beside it and moved in at the end:
    // `Export`'s `Drop` is what wipes these two maps, so a store that refuses
    // partway would otherwise free every Credential read before it untouched.
    let mut gathered = Export {
        version: CURRENT_VERSION,
        registry: registry.clone(),
        profiles: BTreeMap::new(),
    };

    for account in &registry.accounts {
        if crate::holdings::slug(account.key()).is_empty() {
            continue;
        }
        let bundle = account
            .provider()
            .adapter()
            .snapshot(host, &registry.profile_context(host, account)?)
            .map_err(|error| {
                error.with_note(&format!(
                    "{}'s Credential could not be read, so no Export was written.",
                    account.key()
                ))
            })?;
        gathered.profiles.insert(account.key().to_string(), bundle);
    }

    Ok(gathered)
}

impl Export {
    pub fn payloads(
        &self,
    ) -> [(
        &'static str,
        &BTreeMap<String, crate::providers::provider::ProfileBundle>,
    ); 1] {
        [("a Profile", &self.profiles)]
    }
    pub fn profile_for(&self, key: &str) -> Option<&crate::providers::provider::ProfileBundle> {
        self.profiles
            .iter()
            .find(|(held, _)| name::same_name(held, key))
            .map(|(_, bundle)| bundle)
    }

    /// How many Accounts traveled in it.
    pub fn accounts(&self) -> usize {
        self.registry.accounts.len()
    }

    /// The Accounts it holds no Credential for, in the order they are listed.
    ///
    /// Ordinary for a Quarantined Account whose stores hold nothing, and news
    /// for any other, which is why the caller says it rather than this deciding.
    pub fn without_a_credential(&self) -> Vec<&str> {
        self.registry
            .accounts
            .iter()
            .map(Account::key)
            .filter(|email| {
                self.profile_for(email)
                    .is_none_or(|bundle| !bundle.has_credentials())
            })
            .collect()
    }
}

/// The `age` file, as the text that goes in it.
///
/// **Armored**, which is `age`'s own text encoding of the same file: the result
/// is a `str`, so it goes through the Host port's private write like every other
/// file Perch creates rather than through a second, bytes-shaped one.
pub fn seal(export: &Export, passphrase: &str) -> Result<String> {
    for bundle in export.profiles.values() {
        bundle.validate()?;
    }
    // Serialized into a buffer this function owns and wipes. This is every
    // Credential on the machine in cleartext, and freed heap outlives the
    // process in a core dump, a swap file or a hibernation image.
    let mut plain = Wiping::with_room_for(export);
    serde_json::to_writer(&mut plain, export)
        .map_err(|err| PerchError::Other(format!("could not serialize the Export: {err}")))?;

    age::encrypt_and_armor(&recipient(passphrase), &plain.held)
        .map_err(|err| PerchError::Other(format!("could not encrypt the Export: {err}")))
}

/// A buffer that wipes whatever it abandons on the way to being big enough.
///
/// The one thing a plain `Zeroizing<Vec<u8>>` cannot promise: it wipes the buffer
/// it is *holding* and says nothing about the ones the `Vec` outgrew and freed,
/// each of which holds a prefix of every Credential on the machine.
struct Wiping {
    held: Zeroizing<Vec<u8>>,
}

impl Wiping {
    /// Sized so the ordinary Export never grows at all: the serialized form is
    /// a little larger than the sum of what it carries, and this is generous
    /// about "a little". Growing is still handled, because a guess is a guess.
    fn with_room_for(export: &Export) -> Self {
        let carried: usize = export
            .payloads()
            .iter()
            .flat_map(|(_, held)| held.values())
            .map(crate::providers::provider::ProfileBundle::bytes)
            .sum();
        Self {
            held: Zeroizing::new(Vec::with_capacity(carried * 2 + 8 * 1024)),
        }
    }
}

impl std::io::Write for Wiping {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.held.len() + bytes.len() > self.held.capacity() {
            let wanted = (self.held.capacity() * 2).max(self.held.len() + bytes.len());
            let mut grown = Vec::with_capacity(wanted);
            grown.extend_from_slice(&self.held);
            // The move is made here rather than left to `Vec`, which would free
            // the old block untouched. This is the whole point of the type.
            let mut abandoned = std::mem::replace(&mut *self.held, grown);
            abandoned.zeroize();
        }
        self.held.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The other direction: what an `age` file holds, given the passphrase it was
/// sealed with.
///
/// Four ways it can refuse, told apart because they ask for four different next
/// moves — see [`would_not_open`].
pub fn unseal(sealed: &str, passphrase: &str) -> Result<Export> {
    let mut identity = age::scrypt::Identity::new(secret(passphrase));

    // Fixed rather than left to `age`, whose own bound is measured on the
    // machine doing the *decryption*: left alone, whether an Export opens is a
    // question about the pair of machines it traveled between.
    identity.set_max_work_factor(MAX_WORK_FACTOR);

    // Only the buffer handed back, unlike `seal`, which owns the one it fills:
    // whatever `age::decrypt` grew and freed on the way is inside that crate and
    // not Perch's to wipe. The two directions look symmetrical and are not.
    let plain = Zeroizing::new(age::decrypt(&identity, sealed.as_bytes()).map_err(would_not_open)?);

    // Both versions first, off a shape that is only the versions: a newer Perch
    // writes values this build has no variant for, and serde's words about one
    // say the backup is unreadable when it is perfectly well-formed.
    refuse_a_newer_perch(&plain)?;

    let export: Export = serde_json::from_slice(&plain).map_err(|err| PerchError::Malformed {
        path: "the Export".to_string(),
        detail: err.to_string(),
    })?;

    for bundle in export.profiles.values() {
        bundle.validate()?;
    }
    Ok(export)
}

/// Why `age` would not open the file, as something the reader can act on: type
/// it again, stop because no passphrase will open this one, find a machine with
/// more to spend, or this was never an Export. Apart from [`unseal`] to be
/// asserted on directly, two arriving only from files costing seconds of scrypt.
fn would_not_open(err: age::DecryptError) -> PerchError {
    match err {
        // The only answer that genuinely means "that was the wrong passphrase".
        age::DecryptError::DecryptionFailed => PerchError::Invalid(
            "That is not the passphrase this file was written with.".to_string(),
        ),
        // An `age` file encrypted to a key rather than to a passphrase — `age
        // -r` rather than `age -p`. Told as a wrong passphrase, it invites
        // somebody to retype forever one that was never involved.
        age::DecryptError::NoMatchingKeys => PerchError::Invalid(
            "This file was not written with a passphrase, so it is not an Export.".to_string(),
        ),
        // The file is intact and the passphrase may well be right: nothing here
        // is worth typing again, and everything here is worth trying on a
        // machine with more to spend.
        age::DecryptError::ExcessiveWork { required, .. } => PerchError::Invalid(format!(
            "This file takes 2^{required} scrypt rounds to open, more than Perch \
             spends. `age -d` opens it."
        )),
        // This *is* the Export and it did not come through intact: a header
        // whose MAC fails, or a payload that stops early. Its own answer, or a
        // damaged copy of the right file sends somebody looking for another.
        damaged @ (age::DecryptError::InvalidMac | age::DecryptError::Io(_)) => {
            PerchError::Invalid(format!(
                "This `age` file did not come through intact ({damaged}). Find \
                 another copy."
            ))
        }
        other => PerchError::Invalid(format!("This is not an `age` file Perch can read: {other}")),
    }
}

/// The two versions an Export carries, read on their own.
///
/// A shape holding one number deserializes out of any JSON object that carries
/// it, whatever else the object holds, which is the point. An absent version is
/// "it does not say", and the caller reads the document properly next.
fn refuse_a_newer_perch(plain: &[u8]) -> Result<()> {
    #[derive(Deserialize)]
    struct JustTheVersion {
        version: Option<serde_json::Value>,
    }

    #[derive(Deserialize)]
    struct Versioned {
        version: Option<serde_json::Value>,
        registry: Option<JustTheVersion>,
    }

    let Ok(versioned) = serde_json::from_slice::<Versioned>(plain) else {
        return Ok(());
    };

    // A `u64` for `error::claimed_version`'s reason: typed as `u32`, the one
    // number this guard is for fails to deserialize and the guard passes.
    let claimed = |version: Option<serde_json::Value>| version.and_then(|it| it.as_u64());

    let outer = claimed(versioned.version);
    if outer.is_some_and(|claimed| claimed > u64::from(CURRENT_VERSION)) {
        return Err(crate::error::written_by_a_newer_perch(
            "This Export",
            "export",
            outer.unwrap_or_default(),
            CURRENT_VERSION,
        ));
    }
    // The floor the Registry inside holds. An Export can be written by hand with
    // `age -a -p`, and a version below the earliest one names no shape.
    if outer.is_some_and(|claimed| claimed < u64::from(EARLIEST_VERSION)) {
        return Err(if outer == Some(0) {
            no_perch_wrote(outer)
        } else {
            PerchError::Invalid(format!(
                "This Export uses export version {}, which this build cannot restore. Open it with the Perch build that wrote that version. Nothing was imported.",
                outer.unwrap_or_default()
            ))
        });
    }

    // The Registry travels inside carrying its own version, and it is the half
    // that holds the enums — so it is the likelier of the two to be what this
    // build cannot read.
    let inside = claimed(versioned.registry.and_then(|registry| registry.version));
    if inside.is_some_and(|claimed| claimed > u64::from(crate::registry::CURRENT_VERSION)) {
        return Err(crate::error::written_by_a_newer_perch(
            "The Registry inside this Export",
            "Registry",
            inside.unwrap_or_default(),
            crate::registry::CURRENT_VERSION,
        ));
    }
    if inside != Some(u64::from(crate::registry::CURRENT_VERSION)) {
        return Err(PerchError::Invalid("The Registry in this Export uses an unsupported layout. Open it with the Perch build that wrote it; this build requires a fresh installation.".into()));
    }
    Ok(())
}

/// The refusal for an Export claiming a version below the earliest any Perch
/// stamped, which is the Registry's own sentence said about the file around it.
fn no_perch_wrote(claimed: Option<u64>) -> PerchError {
    PerchError::Malformed {
        path: "the Export".to_string(),
        detail: format!(
            "it is export version {}, which no Perch has written.",
            claimed.unwrap_or_default(),
        ),
    }
}

fn recipient(passphrase: &str) -> age::scrypt::Recipient {
    let mut recipient = age::scrypt::Recipient::new(secret(passphrase));
    recipient.set_work_factor(WORK_FACTOR);
    recipient
}

fn secret(passphrase: &str) -> SecretString {
    SecretString::from(passphrase.to_owned())
}

#[cfg(test)]
mod tests {
    use crate::test_support::AccountStoreFixture as _;
    /// Driven past the reserve deliberately: the ordinary Export never grows,
    /// which is why the growing path is the one nothing would otherwise
    /// exercise.
    #[test]
    fn a_document_larger_than_the_buffer_reserved_for_it_is_still_written_whole() {
        use std::io::Write;

        let mut buffer = Wiping {
            held: Zeroizing::new(Vec::with_capacity(8)),
        };
        let written = "every Credential on the machine, at length. ".repeat(200);

        for piece in written.as_bytes().chunks(7) {
            buffer
                .write_all(piece)
                .expect("a buffer that always accepts");
        }
        buffer.flush().expect("nothing to flush");

        assert_eq!(
            std::str::from_utf8(&buffer.held).expect("what went in"),
            written,
            "everything written comes back, however many times it grew"
        );
        assert!(
            buffer.held.capacity() > 8,
            "and it did grow, or this asserted nothing"
        );
    }

    /// The reserve is measured from what the Export actually carries, so the
    /// ordinary one never reaches the growing path at all.
    #[test]
    fn an_ordinary_export_is_written_without_the_buffer_growing_once() {
        let export = Export {
            version: CURRENT_VERSION,
            registry: crate::registry::Registry::default(),
            profiles: BTreeMap::from([(
                "someone@example.com".into(),
                fixture_bundle(Some("a credential"), Some(r#"{"oauthAccount":{}}"#)),
            )]),
        };

        let buffer = Wiping::with_room_for(&export);
        let reserved = buffer.held.capacity();
        let sealed = seal(&export, "correct horse battery staple").expect("it seals");

        assert!(!sealed.is_empty());
        assert!(
            reserved > 8 * 1024,
            "the reserve is generous about a document nobody can size in advance"
        );
    }

    use super::*;
    use crate::domain::Identity;
    use crate::registry::Quarantine;
    use fixtures::{exported_artifact, fixture_bundle};

    const PASSPHRASE: &str = "correct horse battery staple";

    fn an_export() -> Export {
        let mut registry = Registry::default();
        registry.upsert(Account {
            storage_key: None,
            provider: crate::providers::provider::Id::Claude,
            provider_identity: None,
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: Some("Acme".into()),
                organization_uuid: None,
            },
            plan: Some("pro".into()),
            disabled: false,
            quarantine: Some(Quarantine::RenewalRejected),
            group: Some("work".into()),
            utilization: None,
        });
        registry.declare_group("work").expect("a usable name");
        registry
            .name_account("overflow", "someone@example.com")
            .expect("the name is free");

        Export {
            version: CURRENT_VERSION,
            registry,
            profiles: BTreeMap::from([(
                "someone@example.com".into(),
                fixture_bundle(
                    Some(r#"{"claudeAiOauth":{"refreshToken":"sk-ant-ort01-test"}}"#),
                    Some(r#"{"oauthAccount":{"emailAddress":"someone@example.com"}}"#),
                ),
            )]),
        }
    }

    /// Two things say the `age` half. The armor header is what `age -d`
    /// recognizes the text encoding by, and the recipient being scrypt is what
    /// makes it a file `age` opens by *asking for a passphrase*. Verified against
    /// `age` 1.3.1 by hand, which is as far as a test on a machine with no `age`
    /// installed can carry it.
    #[test]
    fn an_export_survives_being_sealed_and_opened_again() {
        let export = an_export();
        let sealed = seal(&export, PASSPHRASE).expect("it seals");

        assert!(
            sealed.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"),
            "an `age` file, in `age`'s own text encoding: {}",
            &sealed[..sealed.len().min(80)],
        );
        let file = age::Decryptor::new_buffered(age::armor::ArmoredReader::new(sealed.as_bytes()))
            .expect("`age`'s own parser reads it as an age file");
        assert!(
            file.is_scrypt(),
            "a passphrase is the recipient, so `age -d` asks for one"
        );

        assert!(
            !sealed.contains("sk-ant-ort01-test") && !sealed.contains("someone@example.com"),
            "nothing in the file is readable without the passphrase"
        );
        assert_eq!(unseal(&sealed, PASSPHRASE).expect("it opens"), export);
    }

    /// An address no Profile could be named after has no store, and a Purge is
    /// what takes one back out — after offering an Export first.
    #[test]
    fn an_address_no_profile_could_be_named_after_does_not_stop_a_gather() {
        let host = crate::host::FakeHost::new();
        let mut registry = crate::registry::Registry::default();
        registry.upsert(crate::cycle::tests::account("one@example.com", vec![]));
        registry.upsert(crate::cycle::tests::account("@", vec![]));

        let gathered = gather(&host, &registry).expect("`@` names no store to read");

        assert_eq!(gathered.accounts(), 2, "both travel in the registry");
        assert!(
            gathered.without_a_credential().contains(&"@"),
            "and the one with no store to read is reported as holding none: {:?}",
            gathered.without_a_credential()
        );
    }

    /// Asserted through the ceiling, because that is the one place `age` will
    /// say a number out loud: opened against a maximum one below what `seal`
    /// spends, the refusal names the work the file actually required.
    #[test]
    fn an_export_is_sealed_with_the_work_perch_chose_rather_than_what_the_machine_could_spare() {
        let sealed = seal(&an_export(), PASSPHRASE).expect("it seals");

        let mut identity = age::scrypt::Identity::new(secret(PASSPHRASE));
        identity.set_max_work_factor(WORK_FACTOR - 1);
        let refused = age::decrypt(&identity, sealed.as_bytes())
            .expect_err("a ceiling below what it was sealed with will not open it");

        match refused {
            age::DecryptError::ExcessiveWork { required, .. } => {
                assert_eq!(
                    required, WORK_FACTOR,
                    "the work factor is pinned, not measured"
                );
            }
            other => panic!("the refusal says how much work the file wants: {other}"),
        }
    }

    /// The one shape in Perch carrying every Credential on the machine at once,
    /// and it derives `PartialEq` — so an `assert_eq!` that fails prints whatever
    /// `Debug` renders. `Credential` and `StoredCredential` write theirs by hand
    /// for the same reason.
    #[test]
    fn what_an_export_holds_is_never_rendered_by_debugging_it() {
        let rendered = format!("{:?}", an_export());

        assert!(
            !rendered.contains("sk-ant-ort01-test") && !rendered.contains("claudeAiOauth"),
            "no Credential, and nothing of one: {rendered}"
        );
        assert!(
            rendered.contains("someone@example.com"),
            "which Accounts it holds one for is the question somebody debugging \
             an Export actually has: {rendered}"
        );
    }

    /// A document that says nothing about Credentials is not an Export of a
    /// machine whose Accounts were all Quarantined — that one says so, with an
    /// empty map. Read as the same thing, it places nothing and reports every
    /// Account restored without a Credential on the way to exit 0.
    #[test]
    fn a_document_that_says_nothing_about_credentials_is_not_an_export() {
        let mut document = serde_json::to_value(an_export()).expect("it serializes");
        document
            .as_object_mut()
            .expect("an Export is an object")
            .remove("profiles");
        let sealed =
            age::encrypt_and_armor(&recipient(PASSPHRASE), document.to_string().as_bytes())
                .expect("it seals");

        let refused = unseal(&sealed, PASSPHRASE).expect_err("it holds no Credentials");

        assert!(
            refused.to_string().contains("profiles"),
            "and it names what is missing: {refused}"
        );

        // The neighboring shape that *is* meaningful, and still opens. Emptied
        // rather than built with `..an_export()`, because a type with a `Drop`
        // cannot have its fields moved out.
        let mut none_kept = an_export();
        none_kept.profiles = BTreeMap::new();
        let sealed = seal(&none_kept, PASSPHRASE).expect("it seals");
        assert_eq!(
            unseal(&sealed, PASSPHRASE).expect("an Export of Quarantined Accounts opens"),
            none_kept
        );
    }

    /// A forgotten passphrase means the Export is gone and re-login is the only
    /// path back. What matters here is that it is *said*, rather than reported
    /// as a corrupt file.
    #[test]
    fn the_wrong_passphrase_opens_nothing_and_says_which_failure_it_is() {
        let sealed = seal(&an_export(), PASSPHRASE).expect("it seals");

        let refused = unseal(&sealed, "not it").expect_err("nothing opens with the wrong one");
        assert!(refused.to_string().contains("passphrase"), "{refused}");

        let refused = unseal("not an age file at all", PASSPHRASE).expect_err("nor does this");
        assert!(refused.to_string().contains("`age` file"), "{refused}");
    }

    /// Being told to go and find a different file — because this one is "not an
    /// `age` file" — is the one answer that is definitely wrong here. Sealed and
    /// then cut rather than asserted against the mapping: a payload that stops
    /// early is the ordinary way an Export is damaged, and costs one scrypt to
    /// produce honestly.
    #[test]
    fn an_export_that_did_not_come_through_intact_is_said_to_be_the_export() {
        let sealed = seal(&an_export(), PASSPHRASE).expect("it seals");
        let cut = &sealed[..sealed.len() * 3 / 4];

        let refused = unseal(cut, PASSPHRASE).expect_err("three quarters of a file opens nothing");

        assert!(
            !refused.to_string().contains("not the passphrase"),
            "the passphrase was right and retyping it will not help: {refused}"
        );
        assert!(
            !refused.to_string().contains("not an `age` file"),
            "it is exactly an `age` file, which is why finding another copy of \
             it is the next move: {refused}"
        );
        assert!(
            refused.to_string().contains("intact"),
            "and it says which of the two happened: {refused}"
        );
    }

    /// Asserted against the mapping rather than against a sealed file, because
    /// manufacturing either costs seconds of scrypt: an Export encrypted to an
    /// X25519 recipient, and one sealed above the ceiling.
    #[test]
    fn a_file_that_is_intact_is_never_reported_as_a_wrong_passphrase() {
        let no_passphrase = would_not_open(age::DecryptError::NoMatchingKeys);
        assert!(
            !no_passphrase.to_string().contains("not the passphrase"),
            "an `age` file written to a key is not a passphrase to retype: \
             {no_passphrase}"
        );
        assert!(
            no_passphrase
                .to_string()
                .contains("not written with a passphrase"),
            "{no_passphrase}"
        );

        let too_much_work = would_not_open(age::DecryptError::ExcessiveWork {
            required: MAX_WORK_FACTOR + 1,
            target: MAX_WORK_FACTOR - 4,
        });
        let said = too_much_work.to_string();
        assert!(
            !said.contains("not an `age` file") && !said.contains("not the passphrase"),
            "an intact Export this machine will not spend the work on is neither \
             of those: {said}"
        );
        assert!(
            said.contains("age -d"),
            "and it says what will open it: {said}"
        );
    }

    /// The Alias, the Group, whether Cycling may choose it and the reason it is
    /// Quarantined are what make a restore arrive with the setup the user had
    /// rather than a pile of nameless logins.
    #[test]
    fn everything_the_registry_says_about_an_account_travels_with_it() {
        let export = an_export();
        let back =
            unseal(&seal(&export, PASSPHRASE).expect("it seals"), PASSPHRASE).expect("it opens");

        let account = back
            .registry
            .account("someone@example.com")
            .expect("the Account is there");
        assert_eq!(account.quarantine, Some(Quarantine::RenewalRejected));
        assert_eq!(account.group.as_deref(), Some("work"));
        assert_eq!(
            back.registry.alias_of("someone@example.com"),
            Some("overflow")
        );
        assert!(back.registry.group("work").is_some());
    }

    /// Nothing has ever written a version other than the current one: this is
    /// about a machine holding two builds, not about the past.
    #[test]
    fn an_export_from_a_newer_perch_is_refused_rather_than_guessed_at() {
        let mut ahead = an_export();
        ahead.version = CURRENT_VERSION + 1;
        let sealed = seal(&ahead, PASSPHRASE).expect("it seals");

        let refused = unseal(&sealed, PASSPHRASE).expect_err("this build does not understand it");
        assert!(refused.to_string().contains("Upgrade Perch"), "{refused}");
    }

    /// The test above bumps an integer on this build's own shape, which is the
    /// one way the case never arrives. A newer Perch writes a value this build
    /// has no variant for — a Strategy it added, a Quarantine reason — so what
    /// turns up is perfectly valid JSON that will not deserialize here.
    #[test]
    fn an_export_this_build_cannot_parse_is_still_refused_as_a_newer_perchs() {
        let ahead = format!(
            r#"{{"version":{},"registry":{{"version":{},"accounts":[{{"identity":{{"email":"someone@example.com"}},"quarantine":"SomethingThisBuildHasNeverHeardOf"}}]}},"credentials":{{}}}}"#,
            CURRENT_VERSION + 1,
            crate::registry::CURRENT_VERSION + 1,
        );
        let sealed = age::encrypt_and_armor(&recipient(PASSPHRASE), ahead.as_bytes())
            .expect("the fixture seals");

        let refused = unseal(&sealed, PASSPHRASE).expect_err("this build does not understand it");

        assert!(refused.to_string().contains("Upgrade Perch"), "{refused}");
        assert!(
            !refused.to_string().contains("not valid JSON"),
            "a file that is perfectly good JSON is not reported as corrupt: {refused}"
        );
    }

    /// The floor, on the half that has one. An Import writes what it read back
    /// out at the current version, so a Registry claiming a version no Perch
    /// stamped would be relabeled rather than refused.
    #[test]
    fn a_registry_claiming_a_version_no_perch_wrote_is_refused_inside_an_export_too() {
        for claimed in ["0", "null"] {
            let bare = format!(
                r#"{{"version":{CURRENT_VERSION},"registry":{{"version":{claimed},"accounts":[]}},"credentials":{{}}}}"#
            );
            let sealed =
                age::encrypt_and_armor(&recipient(PASSPHRASE), bare.as_bytes()).expect("it seals");

            let refused = unseal(&sealed, PASSPHRASE).expect_err("no Perch wrote that");
            assert!(
                refused.to_string().contains("unsupported layout"),
                "{refused}"
            );
        }
    }

    /// The one number this guard exists to name. Typed as a `u32` the envelope
    /// failed to deserialize at all, the guard returned `Ok`, and the reader was
    /// handed serde's words about an integer that will not fit.
    #[test]
    fn an_export_claiming_a_version_above_this_builds_ceiling_still_reads_as_a_newer_perch() {
        for claimed in [u64::from(u32::MAX) + 1, u64::MAX] {
            let ahead = format!(
                r#"{{"version":{claimed},"registry":{{"version":1,"accounts":[]}},"credentials":{{}}}}"#
            );
            let sealed = age::encrypt_and_armor(&recipient(PASSPHRASE), ahead.as_bytes())
                .expect("the fixture seals");

            let refused = unseal(&sealed, PASSPHRASE).expect_err("nothing here wrote that");
            assert!(refused.to_string().contains("Upgrade Perch"), "{refused}");
            assert!(
                !refused.to_string().contains("expected u32"),
                "and not as arithmetic about the number: {refused}"
            );
        }
    }

    /// The envelope had a ceiling and no floor, though `age -a -p` is a
    /// documented way to write one of these by hand and the Registry inside
    /// holds both.
    #[test]
    fn an_export_claiming_a_version_no_perch_wrote_is_refused_like_the_registry_inside_it() {
        let bare = r#"{"version":0,"registry":{"version":1,"accounts":[]},"credentials":{}}"#;
        let sealed =
            age::encrypt_and_armor(&recipient(PASSPHRASE), bare.as_bytes()).expect("it seals");

        let refused = unseal(&sealed, PASSPHRASE).expect_err("no Perch wrote that");
        assert!(
            refused.to_string().contains("no Perch has written"),
            "{refused}"
        );
    }

    /// The Registry inside carries its own version, and it is the half holding
    /// the enums — so it is the likelier of the two to be unreadable here.
    #[test]
    fn a_registry_from_a_newer_perch_inside_a_readable_envelope_is_refused_too() {
        let ahead = format!(
            r#"{{"version":{CURRENT_VERSION},"registry":{{"version":{},"accounts":[]}},"credentials":{{}}}}"#,
            crate::registry::CURRENT_VERSION + 1,
        );
        let sealed = age::encrypt_and_armor(&recipient(PASSPHRASE), ahead.as_bytes())
            .expect("the fixture seals");

        let refused = unseal(&sealed, PASSPHRASE).expect_err("the registry inside is newer");

        assert!(refused.to_string().contains("Upgrade Perch"), "{refused}");
    }

    /// Gathering reads what is stored and asks Anthropic nothing: the fake Host
    /// has no network at all, so a Renewal on the way past would fail here
    /// rather than quietly passing.
    #[test]
    fn gathering_reads_every_store_and_renews_nothing() {
        let host = crate::host::FakeHost::new();
        let mut registry = Registry::default();
        for email in ["one@example.com", "two@example.com"] {
            registry.upsert(Account {
                storage_key: None,
                provider: crate::providers::provider::Id::Claude,
                provider_identity: None,
                identity: Identity {
                    email: email.into(),
                    account_uuid: None,
                    organization_name: None,
                    organization_uuid: None,
                },
                plan: None,
                disabled: false,
                quarantine: None,
                group: None,
                utilization: None,
            });
        }
        let store = registry
            .account("one@example.com")
            .unwrap()
            .store(&host)
            .unwrap();
        host.set_keychain_item(&store.keychain_service, &store.keychain_account, "held");

        let export = gather(&host, &registry).expect("both stores answer");

        assert_eq!(
            exported_artifact(&export, "one@example.com", "oauth").unwrap(),
            "held"
        );
        assert_eq!(
            export.without_a_credential(),
            vec!["two@example.com"],
            "an Account whose stores hold nothing is listed and carries no Credential"
        );
        assert!(host.http_calls().is_empty());
    }

    /// A locked keychain is not "this Account has no Credential": the Export
    /// that recorded it as one would restore to a machine of logins that do not
    /// work, and the user would find out on the day they needed it.
    #[test]
    fn a_store_that_will_not_say_what_it_holds_stops_the_whole_export() {
        let host = crate::host::FakeHost::new();
        let mut registry = Registry::default();
        let account = Account {
            storage_key: None,
            provider: crate::providers::provider::Id::Claude,
            provider_identity: None,
            identity: Identity {
                email: "one@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        };
        // Stored before the lock: the lock is reached only for an item that is
        // found, and a name holding nothing answers "no such item" through it.
        let store = account.store(&host).expect("the Profile can be named");
        host.set_keychain_item(&store.keychain_service, &store.keychain_account, "held");
        host.lock_keychain("User interaction is not allowed");
        registry.upsert(account);

        let refused = gather(&host, &registry).expect_err("nothing can be read");
        assert!(refused.to_string().contains("one@example.com"), "{refused}");
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    pub fn fixture_bundle(
        credential: Option<&str>,
        config: Option<&str>,
    ) -> crate::providers::provider::ProfileBundle {
        let mut artifacts = serde_json::Map::new();
        if let Some(content) = credential {
            artifacts.insert(
                "oauth".into(),
                serde_json::json!({"purpose":"credential", "content":content}),
            );
        }
        if let Some(content) = config {
            artifacts.insert(
                ".claude.json".into(),
                serde_json::json!({"purpose":"configuration", "content":content}),
            );
        }
        serde_json::from_value(serde_json::json!({"artifacts":artifacts})).unwrap()
    }
    pub fn exported_artifact(export: &super::Export, key: &str, name: &str) -> Option<String> {
        serde_json::to_value(export.profile_for(key)?).ok()?["artifacts"][name]["content"]
            .as_str()
            .map(str::to_owned)
    }
}
