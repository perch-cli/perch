//! Everything Perch holds, as one `age` file (ADR 0014).
//!
//! Two halves, and they are separate on purpose. **Gathering** reads the
//! registry and every Credential out of the stores they live in — an effect,
//! and the only part of an Export that touches the machine. **Sealing** turns
//! what was gathered into the bytes that go in the file, and is arithmetic:
//! given an Export and a passphrase it is the same answer on every machine, so
//! it is testable without one.
//!
//! An Export takes everything and has no target. Restoring the Credentials
//! alone would leave a new machine holding working Accounts stripped of every
//! name and rule the user gave them, and a per-Account form is a partial
//! restore — the failure this file exists to prevent, wearing a feature's
//! clothes.

use std::collections::BTreeMap;

use age::secrecy::SecretString;
use serde::{Deserialize, Serialize};

use crate::credentials;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{Account, Registry};

/// The version this build writes, and the only one there has ever been.
///
/// A guard against the future rather than a migration story, as the registry's
/// own is: an Export is meant to outlive the machine it was written on, so the
/// build that reads one back has to be able to say it does not understand it.
/// The registry travels inside carrying its own version, which answers the same
/// question about its own shape; this one is about the envelope around it.
pub const CURRENT_VERSION: u32 = 1;

/// An Export, unsealed: what one `age` file holds before it is encrypted and
/// after it is decrypted again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Export {
    pub version: u32,
    /// The whole registry — every Account, its Alias, its Group, whether
    /// Cycling may choose it, why it is Quarantined where it is, and what each
    /// Group carries. Written whole rather than field by field, so a setting
    /// added to a Group is in the next Export without anybody remembering to put
    /// it there.
    pub registry: Registry,
    /// Every Credential Perch holds, by the address of the Account it belongs
    /// to. An Account whose stores held nothing is absent from here and still
    /// present in the registry above — which is exactly how a Quarantined
    /// Account travels, reason and all.
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
}

/// Reads everything Perch holds: the registry it was handed, and the Credential
/// in each Account's Credential Store.
///
/// Nothing is Renewed and nothing is Rotated. An Export reads what is stored —
/// a Renewal may Rotate, and a file that retired the refresh token of every
/// Account on the way to recording it would have broken the machine it was taken
/// from.
///
/// A store that is there and will not say what it holds stops the whole Export.
/// Reporting an Account with no Credential where the truth is a locked keychain
/// would write a file that restores to a machine of logins that do not work,
/// and the user would find out on the day they needed it.
pub fn gather(host: &dyn Host, registry: &Registry) -> Result<Export> {
    let mut credentials = BTreeMap::new();
    for account in &registry.accounts {
        if let Some(credential) = read_the_credential(host, account)? {
            credentials.insert(account.email().to_string(), credential);
        }
    }

    Ok(Export {
        version: CURRENT_VERSION,
        registry: registry.clone(),
        credentials,
    })
}

fn read_the_credential(host: &dyn Host, account: &Account) -> Result<Option<String>> {
    let store = account.store(host)?;
    let held = credentials::read(host, &store).map_err(|error| {
        error.with_note(&format!(
            "Nothing was written. An Export that left {} out would be a partial \
             restore, which is the whole of what this file exists to prevent.",
            account.email(),
        ))
    })?;
    Ok(held.map(|held| held.credential))
}

impl Export {
    /// How many Accounts travelled in it.
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
            .map(Account::email)
            .filter(|email| !self.credentials.contains_key(*email))
            .collect()
    }
}

/// The `age` file, as the text that goes in it.
///
/// **`age`, taken as a crate** (ADR 0025): encryption sits on neither of Perch's
/// seams — it is not an effect the Host port carries, and it is not shared with
/// Claude Code, so nothing here has to be bug-compatible with anything. What
/// decided it is that the result can be decrypted by the standard `age` command:
/// this file is meant to outlive the machine it was written on, and one readable
/// only by the tool that wrote it is a worse backup than one whose format
/// somebody else maintains.
///
/// **Armored**, which is `age`'s own text encoding of the same file and is read
/// back by the same `age -d`. Two things fall out of it and both are wanted: the
/// result is a `str`, so it goes through the Host port's private write like
/// every other file Perch creates rather than needing a second, bytes-shaped way
/// to write one; and an Export is something a person may want to paste into a
/// password manager, which is a thing you can do with text.
pub fn seal(export: &Export, passphrase: &str) -> Result<String> {
    let plain = serde_json::to_vec(export)
        .map_err(|err| PerchError::Other(format!("could not serialise the Export: {err}")))?;

    age::encrypt_and_armor(&recipient(passphrase), &plain)
        .map_err(|err| PerchError::Other(format!("could not encrypt the Export: {err}")))
}

/// The other direction: what an `age` file holds, given the passphrase it was
/// sealed with.
///
/// Here rather than with whatever comes to read one, because it is the only
/// thing that can say what [`seal`] wrote: a file nothing can open is a file
/// nothing can assert about, and "the whole registry and every Credential are in
/// there" is the whole of what an Export promises.
///
/// A wrong passphrase is told apart from a file that is not an Export at all,
/// because the two have different answers — one is worth typing again and the
/// other is not.
pub fn unseal(sealed: &str, passphrase: &str) -> Result<Export> {
    let identity = age::scrypt::Identity::new(secret(passphrase));
    let plain = age::decrypt(&identity, sealed.as_bytes()).map_err(|err| match err {
        age::DecryptError::NoMatchingKeys | age::DecryptError::DecryptionFailed => {
            PerchError::Invalid(
                "That is not the passphrase this file was written with.".to_string(),
            )
        }
        other => PerchError::Invalid(format!("This is not an `age` file Perch can read: {other}")),
    })?;

    let export: Export = serde_json::from_slice(&plain).map_err(|err| PerchError::Malformed {
        path: "the Export".to_string(),
        detail: err.to_string(),
    })?;

    if export.version > CURRENT_VERSION {
        return Err(crate::error::written_by_a_newer_perch(
            "This Export",
            "export",
            export.version,
            CURRENT_VERSION,
        ));
    }
    Ok(export)
}

fn recipient(passphrase: &str) -> age::scrypt::Recipient {
    age::scrypt::Recipient::new(secret(passphrase))
}

fn secret(passphrase: &str) -> SecretString {
    SecretString::from(passphrase.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::Identity;
    use crate::registry::Quarantine;

    const PASSPHRASE: &str = "correct horse battery staple";

    fn an_export() -> Export {
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: Some("Acme".into()),
                organization_uuid: None,
            },
            plan: Some("pro".into()),
            enabled: true,
            quarantine: Some(Quarantine::RenewalRejected),
            group: Some("work".into()),
            utilization: None,
        });
        registry.declare_group("work").expect("a usable name");
        registry.set_alias("overflow", "someone@example.com");

        Export {
            version: CURRENT_VERSION,
            registry,
            credentials: BTreeMap::from([(
                "someone@example.com".to_string(),
                r#"{"claudeAiOauth":{"refreshToken":"sk-ant-ort01-test"}}"#.to_string(),
            )]),
        }
    }

    /// The whole of what an Export promises: what went in comes back out, and
    /// what it travelled in is a file the standard `age` command reads.
    ///
    /// Two things say the second half. The armor header is what `age -d`
    /// recognises the text encoding by, and the recipient is scrypt — which is
    /// what makes it a file `age` opens by *asking for a passphrase* rather than
    /// one it wants a key file for. Verified against `age` 1.3.1 by hand, which
    /// is as far as a test that must run on a machine with no `age` installed
    /// can carry it.
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

    /// A forgotten passphrase means the Export is gone, and re-login is the only
    /// path back — the trade ADR 0014 made deliberately. What matters here is
    /// that it is *said* rather than reported as a corrupt file.
    #[test]
    fn the_wrong_passphrase_opens_nothing_and_says_which_failure_it_is() {
        let sealed = seal(&an_export(), PASSPHRASE).expect("it seals");

        let refused = unseal(&sealed, "not it").expect_err("nothing opens with the wrong one");
        assert!(refused.to_string().contains("passphrase"), "{refused}");

        let refused = unseal("not an age file at all", PASSPHRASE).expect_err("nor does this");
        assert!(refused.to_string().contains("`age` file"), "{refused}");
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

    /// An Export written by a build that understands more than this one is
    /// refused rather than half-read. Nothing has ever written a version other
    /// than the current one — this is about the future, not the past.
    #[test]
    fn an_export_from_a_newer_perch_is_refused_rather_than_guessed_at() {
        let ahead = Export {
            version: CURRENT_VERSION + 1,
            ..an_export()
        };
        let sealed = seal(&ahead, PASSPHRASE).expect("it seals");

        let refused = unseal(&sealed, PASSPHRASE).expect_err("this build does not understand it");
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
                identity: Identity {
                    email: email.into(),
                    account_uuid: None,
                    organization_name: None,
                    organization_uuid: None,
                },
                plan: None,
                enabled: true,
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

        assert_eq!(export.credentials.get("one@example.com").unwrap(), "held");
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
        let host =
            crate::host::FakeHost::new().with_locked_keychain("User interaction is not allowed");
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: "one@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            enabled: true,
            quarantine: None,
            group: None,
            utilization: None,
        });

        let refused = gather(&host, &registry).expect_err("nothing can be read");
        assert!(refused.to_string().contains("one@example.com"), "{refused}");
        assert!(refused.to_string().contains("partial restore"), "{refused}");
    }
}
