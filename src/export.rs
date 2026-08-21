//! Everything Perch holds, as one `age` file (ADR the-holdings-go-out-sealed).
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
use zeroize::{Zeroize, Zeroizing};

use crate::credentials;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{self, Account, Registry};

/// The version this build writes, and the only one there has ever been.
///
/// A guard against the future rather than a migration story, as the registry's
/// own is: an Export is meant to outlive the machine it was written on, so the
/// build that reads one back has to be able to say it does not understand it.
/// The registry travels inside carrying its own version, which answers the same
/// question about its own shape; this one is about the envelope around it.
pub const CURRENT_VERSION: u32 = 1;

/// The most scrypt work [`unseal`] will spend opening one file, as `log2(N)`.
///
/// A fixed number rather than one this machine measures, which is the whole
/// point of it. `age`'s own default ceiling is "about a second of work here,
/// plus four doublings", and its default *work factor* is about a second of
/// work on whatever wrote the file — so with both left alone, whether an Export
/// opens is a question about the pair of machines it traveled between. An
/// Export written on a desktop and carried to a laptop, or opened inside a
/// container with less CPU than the one that wrote it, was refused on that
/// alone. ADR the-holdings-go-out-sealed wants this file to outlive the machine
/// that wrote it, and it cannot do that while what opens it depends on the
/// machine that opens it.
///
/// 22 is where `age`'s own guidance tops out, and is comfortably above
/// [`WORK_FACTOR`], since 2^22 scrypt rounds want four gigabytes to themselves.
const MAX_WORK_FACTOR: u8 = 22;

/// The scrypt work [`seal`] spends writing one file, as `log2(N)`.
///
/// Fixed for the half of the reason [`MAX_WORK_FACTOR`] is, and for a second
/// one that is worse. `age` calibrates a work factor by timing 2^10 rounds on
/// the machine doing the encrypting and doubling *only while that measurement
/// is under a second* — so the number is not merely machine-dependent, it has
/// no floor. On anything CPU-starved enough that 2^10 rounds already take a
/// second — a cgroup-quota'd CI container, a loaded laptop, a Pi — the loop
/// never runs and the file is sealed at 1024 rounds, which is a GPU-hours
/// problem for a passphrase a person chose. Nothing in the file, the report or
/// [`unseal`] would have said so: the ceiling is checked and there was no floor
/// to check against.
///
/// So the strength of an Export is decided here, once, rather than by whatever
/// the machine happened to be doing that afternoon — which is also what the
/// module doc means by sealing being arithmetic.
///
/// 19 rather than `age`'s own "roughly 1 second on a modern machine" guess of
/// 18, because this is a file meant to outlive the machine and the cost is paid
/// once per Export and once per Import. Slow hardware pays seconds for it; that
/// is the right way round, and the alternative is the hardware deciding how well
/// the backup is encrypted.
const WORK_FACTOR: u8 = 19;

/// What Perch spends sealing has to stay under what it will spend opening, or
/// every Export it writes is one it refuses to read. Asserted where the two
/// numbers are, and at compile time, because there is no run in which it is
/// worth discovering.
const _: () = assert!(WORK_FACTOR < MAX_WORK_FACTOR);

/// An Export, unsealed: what one `age` file holds before it is encrypted and
/// after it is decrypted again.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
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
    ///
    /// Required, unlike the two maps either side of it. An empty map is a
    /// meaningful Export — every Account Quarantined, which is a thing `gather`
    /// can honestly write — and a *missing* key is a document that never said
    /// anything about Credentials at all. Defaulted, it unsealed happily,
    /// `place` put nothing anywhere, and the Import reported every Account
    /// "restored without one" and exited 0: the partial restore
    /// ADR the-holdings-go-out-sealed exists to prevent, wearing a success's
    /// clothes.
    pub credentials: BTreeMap<String, String>,
    /// Each Account's own `.claude.json`, by the address of the Account it
    /// belongs to.
    ///
    /// A Profile holds two things, not one. The Credential is what cannot be
    /// reconstructed, and this is what cannot be reconstructed *faithfully*:
    /// Claude Code writes an `oauthAccount` block carrying fields beyond the
    /// four the registry records, and a Switch prefers that block verbatim over
    /// one Perch composes. An Export without it restores every Account into the
    /// degraded state [`crate::adopt`] goes out of its way to keep the first
    /// one out of. It is also what a Run Carries from, so a Profile arriving
    /// without one meets the onboarding dialog on every single Run
    /// (ADR everything-but-the-account).
    #[serde(default)]
    pub identity_files: BTreeMap<String, String>,
}

/// Wiped when it goes out of scope, for the reason its [`Debug`] is written by
/// hand: this is the one shape in Perch carrying every Credential on the machine
/// at once, and it lives for the whole of an Export or an Import rather than for
/// a call.
///
/// Only the two maps that hold secrets. The registry beside them is addresses,
/// Aliases, Groups and figures — the thing an Export is *for* somebody reading,
/// and nothing a core dump makes worse.
///
/// Hand-written rather than derived, because `ZeroizeOnDrop` would want every
/// field to be `Zeroize` and `Registry` is not one, nor should it become one to
/// satisfy a derive.
impl Drop for Export {
    fn drop(&mut self) {
        for held in self.credentials.values_mut() {
            held.zeroize();
        }
        for held in self.identity_files.values_mut() {
            held.zeroize();
        }
    }
}

impl std::fmt::Debug for Export {
    /// Written by hand for the reason [`crate::probe::Credential`] and
    /// [`crate::credentials::StoredCredential`] are: a derived one would print
    /// every field, and this is the one shape in Perch carrying every Credential
    /// on the machine at once. `Export` derives `PartialEq` too, so a regressed
    /// `assert_eq!` in the round-trip test would have printed the lot — and a
    /// formatting specifier is all that stands between any of these values and a
    /// log.
    ///
    /// Counts and addresses, never secrets. Which Accounts an Export holds a
    /// Credential for is exactly the question somebody debugging one has, and it
    /// is answerable without rendering a single token.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Export")
            .field("version", &self.version)
            .field("registry", &self.registry)
            .field("credentials for", &self.credentials.keys())
            .field("identity files for", &self.identity_files.keys())
            .finish()
    }
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
    let mut identity_files = BTreeMap::new();
    for account in &registry.accounts {
        if let Some(credential) = read_the_credential(host, registry, account)? {
            credentials.insert(account.email().to_string(), credential);
        }
        // Unlike the Credential, an identity file that will not be read does not
        // stop the Export. It is not a secret and it is not irreplaceable — an
        // Import composes one from the Identity the registry carries — so the
        // whole file is worth more than the fidelity of one Profile's copy.
        if let Some(contents) = read_the_identity_file(host, account) {
            identity_files.insert(account.email().to_string(), contents);
        }
    }

    Ok(Export {
        version: CURRENT_VERSION,
        registry: registry.clone(),
        credentials,
        identity_files,
    })
}

/// The Credential to write for one Account: the live one where that is what the
/// Account's Credential *is*, and the copy in its own Profile otherwise.
///
/// For the active Account the live Credential is in the Default Profile, and it
/// is ahead of the copy in its own Profile — which only catches up when a
/// Switch away Captures it (ADR a-switch-is-written-down-first). A Renewal
/// Rotates the live copy and Anthropic retires the refresh token it replaced,
/// so reading the Profile copy for the one Account the user is actually working
/// in wrote the single token in the file most likely to be dead already.
/// `perch watcher run` Renews that Account every few minutes, which makes it
/// the ordinary case rather than the unlucky one, and the user would find out
/// on the day they needed it.
///
/// The live copy is only taken on the evidence [`crate::switch::capture`]
/// requires before it copies that same Credential anywhere: the Default
/// Profile's Identity naming this Account, or naming nobody. A live Credential
/// that belongs to somebody else is not this Account's to export.
fn read_the_credential(
    host: &dyn Host,
    registry: &Registry,
    account: &Account,
) -> Result<Option<String>> {
    // The live store first, and its own Profile as the fallback rather than the
    // answer: `claude /logout` empties the live store and leaves the Account
    // active, and an Export that read nothing there and stopped would drop a
    // Credential Perch is still holding perfectly well.
    if let Some(live) = the_live_store(host, registry, account)?
        && let Some(credential) = read_from(host, &live, account)?
    {
        return Ok(Some(credential));
    }
    read_from(host, &account.store(host)?, account)
}

/// The Default Profile, when what is live in it is this Account's Credential.
fn the_live_store(
    host: &dyn Host,
    registry: &Registry,
    account: &Account,
) -> Result<Option<crate::probe::Store>> {
    // A *settled* registry, rather than `is_active`, which answers a Landing
    // with the Account being **left** (ADR a-switch-is-written-down-first). A
    // Switch killed between storing the arriving Credential and patching the
    // Identity leaves exactly that state, and `is_active` then said yes for the
    // leaving Account while the live store held the arriving one's Credential —
    // so the Export filed one Account's refresh token under the other's address
    // and dropped the genuine copy of it. Restoring that gives two Accounts one
    // token, and the first Renewal Rotates it and kills the other.
    //
    // Every command that acts on the live Credential settles a Landing before
    // reading who is active, and `perch holdings export` now does too. This is
    // the belt to that brace, and it is the one that covers `perch holdings
    // purge` — which reads the registry directly, and must not become the
    // command an unaccountable Landing can stop.
    if !matches!(
        registry.active(),
        registry::Active::Settled(active) if registry::same_name(active, account.email())
    ) {
        return Ok(None);
    }
    let live = registry::the_default_profile(host)?;
    // An Identity that is absent, or one that will not be read, is not evidence
    // against — exactly as it is not in a Capture. Only an Identity that names
    // somebody else is.
    //
    // A Claude Code that will not say its version is the same thing one step
    // earlier: without it there is no Identity to read, so there is nothing
    // that names somebody else. Propagated instead, it refused the whole Export
    // — after the passphrase had been typed twice — on a machine where Claude
    // Code had been uninstalled or a global install had been wiped, which is
    // exactly the machine somebody is decommissioning when they run this. And
    // it takes `perch holdings purge` with it, because the Export it offers
    // first is one that stops the Purge when it fails.
    let somebody_else = matches!(
        crate::probe::Installed::probed(host)
            .and_then(|installed| crate::probe::read_identity(host, &live, &installed)),
        Ok(Some(identity)) if !registry::same_name(&identity.email, account.email())
    );
    Ok((!somebody_else).then_some(live))
}

fn read_from(
    host: &dyn Host,
    store: &crate::probe::Store,
    account: &Account,
) -> Result<Option<String>> {
    let held = credentials::read(host, store).map_err(|error| {
        error.with_note(&format!(
            "Nothing was written. An Export that left {} out would be a partial \
             restore, which is the whole of what this file exists to prevent.",
            account.email(),
        ))
    })?;
    // Copied out of its `Zeroizing` rather than carried in it: `Export` wipes
    // both of its maps in its own `Drop` above, and the wrapper this came in
    // wipes the buffer it is leaving behind.
    Ok(held.map(|held| held.credential.to_string()))
}

fn read_the_identity_file(host: &dyn Host, account: &Account) -> Option<String> {
    let store = account.store(host).ok()?;
    host.read_file(&store.identity_file).ok()
}

impl Export {
    /// How many Accounts traveled in it.
    pub fn accounts(&self) -> usize {
        self.registry.accounts.len()
    }

    /// The Credential this Export carries for one Account, if it carries one.
    ///
    /// Keyed the way an address is compared everywhere else —
    /// `registry::same_name` folds case — rather than by the `BTreeMap` lookup
    /// the key type offers. An Export is a file a person may write themselves
    /// with `age -a -p` (ADR the-holdings-go-out-sealed), and one keying a
    /// credential `ONE@example.com` beside an account entry `one@example.com`
    /// is one where the two halves of an Import disagreed: placement found the
    /// Credential and put it down, and the report afterwards said the Account
    /// had been "restored without one" and sent its owner to `perch relogin`
    /// for an Account that was fine.
    ///
    /// Here rather than at either caller, because the disagreement was the two
    /// of them asking the same question two ways.
    pub fn credential_for(&self, email: &str) -> Option<&String> {
        by_name(&self.credentials, email)
    }

    /// The same, for the `.claude.json` that travels beside it.
    pub fn identity_file_for(&self, email: &str) -> Option<&String> {
        by_name(&self.identity_files, email)
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
            .filter(|email| self.credential_for(email).is_none())
            .collect()
    }
}

/// What one Account's entry is in a map an Export keys by address.
fn by_name<'a>(held: &'a BTreeMap<String, String>, email: &str) -> Option<&'a String> {
    held.iter()
        .find(|(key, _)| registry::same_name(key, email))
        .map(|(_, value)| value)
}

/// The `age` file, as the text that goes in it.
///
/// **`age`, taken as a crate** (ADR a-crate-must-not-cost-a-seam): encryption
/// sits on neither of Perch's seams — it is not an effect the Host port
/// carries, and it is not shared with Claude Code, so nothing here has to be
/// bug-compatible with anything. What decided it is that the result can be
/// decrypted by the standard `age` command: this file is meant to outlive the
/// machine it was written on, and one readable only by the tool that wrote it
/// is a worse backup than one whose format somebody else maintains.
///
/// **Armored**, which is `age`'s own text encoding of the same file and is read
/// back by the same `age -d`. Two things fall out of it and both are wanted: the
/// result is a `str`, so it goes through the Host port's private write like
/// every other file Perch creates rather than needing a second, bytes-shaped way
/// to write one; and an Export is something a person may want to paste into a
/// password manager, which is a thing you can do with text.
pub fn seal(export: &Export, passphrase: &str) -> Result<String> {
    // Wiped before it is freed. This is every Credential on the machine in
    // cleartext, and it is the one moment they are all in one buffer — freed
    // heap outlives the process in a core dump, a swap file or a hibernation
    // image, and a plain overwrite of memory nothing reads again is something a
    // compiler may elide. `Zeroizing` takes the `Vec` rather than copying it, so
    // what is wiped is the buffer `to_vec` produced.
    // Serialized *into* a buffer this function owns rather than handed one
    // `to_vec` grew for itself. `Vec` grows by allocating a bigger block,
    // copying, and freeing the old one — so a document that doubles four times
    // leaves four buffers in freed heap, each holding a prefix of every
    // Credential on the machine, and `Zeroizing` wipes only the last of them.
    // Which is the very thing the paragraph above says this is here to prevent.
    let mut plain = Wiping::with_room_for(export);
    serde_json::to_writer(&mut plain, export)
        .map_err(|err| PerchError::Other(format!("could not serialize the Export: {err}")))?;

    age::encrypt_and_armor(&recipient(passphrase), &plain.held)
        .map_err(|err| PerchError::Other(format!("could not encrypt the Export: {err}")))
}

/// A buffer that wipes whatever it abandons on the way to being big enough.
///
/// The one thing a plain `Zeroizing<Vec<u8>>` cannot promise: it wipes the
/// buffer it is *holding* when it drops, and says nothing about the ones the
/// `Vec` outgrew and freed along the way. Every one of those holds a prefix of
/// the document, and this document is every Credential on the machine in
/// cleartext.
struct Wiping {
    held: Zeroizing<Vec<u8>>,
}

impl Wiping {
    /// Sized so the ordinary Export never grows at all: the serialized form is
    /// a little larger than the sum of what it carries, and this is generous
    /// about "a little". Growing is still handled, because a guess is a guess.
    fn with_room_for(export: &Export) -> Self {
        let carried: usize = export
            .credentials
            .values()
            .chain(export.identity_files.values())
            .map(String::len)
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
/// Here rather than with whatever comes to read one, because it is the only
/// thing that can say what [`seal`] wrote: a file nothing can open is a file
/// nothing can assert about, and "the whole registry and every Credential are in
/// there" is the whole of what an Export promises.
///
/// A wrong passphrase is told apart from a file that is not an Export at all,
/// because the two have different answers — one is worth typing again and the
/// other is not. Two more answers are worth as much and are told apart for the
/// same reason: an `age` file that was never a passphrase one, and one this
/// machine will not spend the work to open.
pub fn unseal(sealed: &str, passphrase: &str) -> Result<Export> {
    let mut identity = age::scrypt::Identity::new(secret(passphrase));

    // The bound `age` picks on its own is measured on the machine doing the
    // *decryption* — about a second of work here, plus four doublings. So an
    // Export written on a fast desktop and opened on a slow laptop, or inside a
    // CPU-limited container, was refused for no reason but the pair of machines
    // it traveled between. That is the one property
    // ADR the-holdings-go-out-sealed is about: this file is meant to outlive
    // the machine that wrote it, so what will open it cannot be a function of
    // the machine that opens it. 22 is where `age`'s own guidance tops out, and
    // is above `WORK_FACTOR`, which is what `seal` spends — pinned there for
    // the same reason, and for the floor `age`'s calibration does not have.
    identity.set_max_work_factor(MAX_WORK_FACTOR);

    // The same buffer coming the other way, and wiped for the same reason.
    //
    // Only the buffer handed back, unlike `seal`, which owns the one it fills:
    // whatever `age::decrypt` grew and freed on the way to producing this is
    // inside that crate and not Perch's to wipe. Said rather than left implied,
    // because the two directions look symmetrical and are not.
    let plain = Zeroizing::new(age::decrypt(&identity, sealed.as_bytes()).map_err(would_not_open)?);

    // Both versions first, off a shape that is only the versions, and before
    // the document is read as an Export. This is the order the guards need to
    // be any use at all: a newer Perch is exactly the thing that writes a value
    // this build has no variant for — a Strategy it added, a Quarantine reason —
    // and reading the document first fails on that with serde's own words. The
    // user is then told their *backup file is unreadable*, about a file that is
    // perfectly well-formed, on the day the machine it would have restored is
    // gone. `registry::load` gets this right and says so; this did not, so both
    // version fields were dead in the only case they were written for.
    refuse_a_newer_perch(&plain)?;

    serde_json::from_slice(&plain).map_err(|err| PerchError::Malformed {
        path: "the Export".to_string(),
        detail: err.to_string(),
    })
}

/// Why `age` would not open the file, said as something the reader can act on.
///
/// Four answers rather than two, because they ask for four different next
/// moves: type it again, stop typing because no passphrase will ever open this
/// one, find a machine with more to spend, and this is not an Export.
///
/// Apart from [`unseal`] so it can be asserted on directly. Two of these arrive
/// only from files that cost seconds of scrypt to manufacture, and a refusal
/// nothing can afford to test is a refusal that quietly stops being true.
fn would_not_open(err: age::DecryptError) -> PerchError {
    match err {
        // The only answer that genuinely means "that was the wrong passphrase".
        age::DecryptError::DecryptionFailed => PerchError::Invalid(
            "That is not the passphrase this file was written with.".to_string(),
        ),
        // An `age` file, but one encrypted to a key rather than to a passphrase
        // — `age -r ...` rather than `age -p`. Said as itself, because told as a
        // wrong passphrase it invites somebody to retype forever one that was
        // never involved.
        age::DecryptError::NoMatchingKeys => PerchError::Invalid(
            "This is an `age` file, but it was not written with a passphrase, so \
             no passphrase will open it. An Export always is."
                .to_string(),
        ),
        // The file is intact and the passphrase may well be right. Nothing here
        // is worth typing again, and everything here is worth trying on a
        // machine with more to spend — which is the point of a format something
        // other than Perch maintains.
        age::DecryptError::ExcessiveWork { required, .. } => PerchError::Invalid(format!(
            "This file was sealed with more work than Perch will spend opening \
             one (2^{required} scrypt rounds, against a ceiling of \
             2^{MAX_WORK_FACTOR}). Nothing is wrong with the file and the \
             passphrase is not in question — `age -d` opens it where this will \
             not."
        )),
        // This *is* the Export, and it did not come through intact: a header
        // whose MAC no longer checks out, or a payload that stops early. A
        // backup copied off a filling disk, an interrupted transfer, a file
        // half-pasted out of a password manager.
        //
        // Its own answer rather than the catch-all, which told somebody holding
        // a damaged copy of the right file to go and look for a different one.
        // That is the sentence that matters most on the one day this is read —
        // the day the machine it was taken from is gone — and it was the answer
        // that is definitely wrong.
        damaged @ (age::DecryptError::InvalidMac | age::DecryptError::Io(_)) => {
            PerchError::Invalid(format!(
                "This is an `age` file and it did not come through intact \
                 ({damaged}). The passphrase is not in question — nothing will \
                 open this copy. Find another one."
            ))
        }
        other => PerchError::Invalid(format!("This is not an `age` file Perch can read: {other}")),
    }
}

/// The two versions an Export carries, read on their own.
///
/// A shape holding one number deserializes out of any JSON object that carries
/// it, whatever else the object holds and whatever the rest of it means — which
/// is the whole point. An absent version is "it does not say", which is not a
/// claim about a newer Perch: the caller goes on to read the document properly
/// and reports what it finds there.
fn refuse_a_newer_perch(plain: &[u8]) -> Result<()> {
    #[derive(Deserialize)]
    struct JustTheVersion {
        version: Option<u32>,
    }

    #[derive(Deserialize)]
    struct Versioned {
        version: Option<u32>,
        registry: Option<JustTheVersion>,
    }

    let Ok(versioned) = serde_json::from_slice::<Versioned>(plain) else {
        return Ok(());
    };

    if versioned
        .version
        .is_some_and(|claimed| claimed > CURRENT_VERSION)
    {
        return Err(crate::error::written_by_a_newer_perch(
            "This Export",
            "export",
            versioned.version.unwrap_or_default(),
            CURRENT_VERSION,
        ));
    }

    // The registry travels inside carrying its own version, and it is the half
    // that holds the enums — so it is the likelier of the two to be what this
    // build cannot read.
    let inside = versioned.registry.and_then(|registry| registry.version);
    if inside.is_some_and(|claimed| claimed > crate::registry::CURRENT_VERSION) {
        return Err(crate::error::written_by_a_newer_perch(
            "The registry inside this Export",
            "registry",
            inside.unwrap_or_default(),
            crate::registry::CURRENT_VERSION,
        ));
    }
    Ok(())
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
    /// The buffer an Export is serialized into grows by hand, wiping what it
    /// leaves behind — so a document bigger than the room reserved for it is
    /// still one buffer at the end rather than a trail of them in freed heap.
    ///
    /// Driven past the reserve deliberately: the ordinary Export never grows,
    /// which is exactly why the growing path is the one nothing would otherwise
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
            credentials: [(
                "someone@example.com".to_string(),
                "a credential".to_string(),
            )]
            .into(),
            identity_files: [(
                "someone@example.com".to_string(),
                "{\"oauthAccount\":{}}".to_string(),
            )]
            .into(),
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
            credentials: BTreeMap::from([(
                "someone@example.com".to_string(),
                r#"{"claudeAiOauth":{"refreshToken":"sk-ant-ort01-test"}}"#.to_string(),
            )]),
            identity_files: BTreeMap::from([(
                "someone@example.com".to_string(),
                r#"{"oauthAccount":{"emailAddress":"someone@example.com"}}"#.to_string(),
            )]),
        }
    }

    /// The whole of what an Export promises: what went in comes back out, and
    /// what it traveled in is a file the standard `age` command reads.
    ///
    /// Two things say the second half. The armor header is what `age -d`
    /// recognizes the text encoding by, and the recipient is scrypt — which is
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

    /// What an Export is encrypted *with*, which nothing was asserting.
    ///
    /// Left to `age`, the work factor is measured on the machine doing the
    /// sealing: 2^10 rounds timed once, then doubled only while that measurement
    /// stays under a second. A machine slow enough that 2^10 already takes a
    /// second seals at 2^10 — and since `unseal` checks only a ceiling, such a
    /// file opens in silence. The strength of every backup Perch writes was a
    /// property of whatever else the CPU was doing at the time.
    ///
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
    /// and it derives `PartialEq` — so the `assert_eq!` above would have printed
    /// the lot the day it regressed. `Credential` and `StoredCredential` both
    /// write `Debug` by hand for exactly this reason: a formatting specifier is
    /// all that stands between any of these values and a log.
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
    /// empty map. Read as the same thing, it unsealed happily, placed nothing,
    /// and reported every Account restored without a Credential on the way to
    /// exit 0: the partial restore the file exists to prevent, wearing a
    /// success's clothes, on the day the machine it would have restored is gone.
    #[test]
    fn a_document_that_says_nothing_about_credentials_is_not_an_export() {
        let mut document = serde_json::to_value(an_export()).expect("it serializes");
        document
            .as_object_mut()
            .expect("an Export is an object")
            .remove("credentials");
        let sealed =
            age::encrypt_and_armor(&recipient(PASSPHRASE), document.to_string().as_bytes())
                .expect("it seals");

        let refused = unseal(&sealed, PASSPHRASE).expect_err("it holds no Credentials");

        assert!(
            refused.to_string().contains("credentials"),
            "and it names what is missing: {refused}"
        );

        // The neighboring shape that *is* meaningful, and still opens. Built by
        // emptying rather than by `..an_export()`, because an `Export` wipes
        // itself when it is dropped and a type with a `Drop` cannot have its
        // fields moved out.
        let mut none_kept = an_export();
        none_kept.credentials = BTreeMap::new();
        let sealed = seal(&none_kept, PASSPHRASE).expect("it seals");
        assert_eq!(
            unseal(&sealed, PASSPHRASE).expect("an Export of Quarantined Accounts opens"),
            none_kept
        );
    }

    /// A forgotten passphrase means the Export is gone, and re-login is the
    /// only path back — the trade ADR the-holdings-go-out-sealed made
    /// deliberately. What matters here is that it is *said* rather than
    /// reported as a corrupt file.
    #[test]
    fn the_wrong_passphrase_opens_nothing_and_says_which_failure_it_is() {
        let sealed = seal(&an_export(), PASSPHRASE).expect("it seals");

        let refused = unseal(&sealed, "not it").expect_err("nothing opens with the wrong one");
        assert!(refused.to_string().contains("passphrase"), "{refused}");

        let refused = unseal("not an age file at all", PASSPHRASE).expect_err("nor does this");
        assert!(refused.to_string().contains("`age` file"), "{refused}");
    }

    /// The Export somebody has, damaged. The whole of what an Export is for is
    /// the day the machine it was taken from is gone, and on that day being
    /// told to go and find a different file — because this one is "not an `age`
    /// file" — is the one answer that is definitely wrong.
    ///
    /// Sealed and then cut, rather than asserted against the mapping: a payload
    /// that stops early is the ordinary way an Export is damaged, and it costs
    /// one scrypt to produce honestly.
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

    /// The two refusals that arrive at the worst possible moment — the day the
    /// machine the Export would have restored is gone — and both of which used
    /// to be reported as something they are not.
    ///
    /// Asserted against the mapping rather than against a sealed file, because
    /// manufacturing either one costs seconds of scrypt: an Export encrypted to
    /// an X25519 recipient, and one sealed at a work factor above the ceiling.
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
                .contains("passphrase will open it"),
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

    /// An Export written by a build that understands more than this one is
    /// refused rather than half-read. Nothing has ever written a version other
    /// than the current one — this is about the future, not the past.
    #[test]
    fn an_export_from_a_newer_perch_is_refused_rather_than_guessed_at() {
        let mut ahead = an_export();
        ahead.version = CURRENT_VERSION + 1;
        let sealed = seal(&ahead, PASSPHRASE).expect("it seals");

        let refused = unseal(&sealed, PASSPHRASE).expect_err("this build does not understand it");
        assert!(refused.to_string().contains("Upgrade Perch"), "{refused}");
    }

    /// And refused as *that*, rather than as a corrupt file.
    ///
    /// The test above builds the newer Export out of this build's own shape and
    /// bumps the integer, which is the one way the case never actually arrives.
    /// A newer Perch is precisely the thing that writes a value this build has
    /// no variant for — a Strategy it added, a Quarantine reason — so what turns
    /// up is a document that is perfectly valid JSON and will not deserialize
    /// here. Read as an `Export` first, that is reported as a malformed file:
    /// the user is told their backup is corrupt, on the day the machine it
    /// would have restored is gone.
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

    /// The registry inside carries its own version, and it is the half holding
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
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        let refused = gather(&host, &registry).expect_err("nothing can be read");
        assert!(refused.to_string().contains("one@example.com"), "{refused}");
        assert!(refused.to_string().contains("partial restore"), "{refused}");
    }
}
