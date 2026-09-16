//! Native Credential placement failures preserve recoverable stores.

use crate::host::{FakeHost, Files, Platform, Refusing};

const EMAIL: &str = "someone@example.com";
const LOGIN_NAME: &str = "someone";
const CREDENTIAL: &str = "synthetic credential";
const STALE: &str = "retired synthetic credential";

fn store_of(host: &FakeHost, email: &str) -> crate::providers::claude::probe::Store {
    crate::providers::claude::probe::store_for_profile(
        host,
        &std::path::Path::new("/profiles").join(crate::holdings::slug(email)),
    )
    .unwrap()
}
fn a_stored_profile() -> FakeHost {
    let host = FakeHost::new();
    let store = store_of(&host, EMAIL);
    super::make_dir(&host, &store.config_dir).unwrap();
    host.set_keychain_item(&store.keychain_service, LOGIN_NAME, CREDENTIAL);
    host
}
fn logged_in_machine_off_macos() -> FakeHost {
    FakeHost::new().with_platform(Platform::Other)
}

/// A Store that will not say what it holds is refused where it may hold
/// something, and let be where it holds nothing under this name.
///
/// The keychain is read first on macOS, so a Credential behind a locked one
/// wins every read after it opens rather than none.
#[test]
fn a_store_that_will_not_answer_is_refused_only_where_it_may_hold_a_credential() {
    let host = a_stored_profile();
    let store = store_of(&host, EMAIL);
    host.lock_keychain("User interaction is not allowed");
    host.forget_notes();

    let refused = super::store_credential(&host, &store, CREDENTIAL)
        .expect_err("a locked keychain may be holding the Credential this replaces");
    let said = refused.to_string();
    assert!(
        said.contains("would not say whether it still holds"),
        "the refusal says what could not be established: {said}"
    );
    assert!(
        said.contains("Open it and run this again"),
        "and the remedy is opening it, not emptying it: {said}"
    );

    // The same lock, for a name the keychain holds nothing under: it answers
    // "no such item" through the lock, and nothing survives the lock opening.
    let untouched = store_of(&host, "nobody@example.com");
    super::make_dir(&host, &untouched.config_dir).expect("the Profile can be made");
    super::store_credential(&host, &untouched, CREDENTIAL)
        .expect("nothing is stored under this name");
}

/// A keychain item is keyed on a Profile directory's path and outlives the
/// directory, so a directory the machine does not have is no evidence about
/// what the keychain holds under its name. The case: `rm -rf ~/.perch` leaves
/// every item behind, and the Import that follows writes the same paths.
#[test]
fn a_profile_the_machine_does_not_have_is_still_refused_where_the_keychain_kept_one() {
    let host = a_stored_profile();
    let store = store_of(&host, EMAIL);

    // The directory goes and the keychain item stays, which is the state a hand
    // removal leaves. Perch has never seen this Profile directory.
    host.remove_dir_all(&store.config_dir)
        .expect("the directory can be taken out from under it");
    assert!(!host.path_exists(&store.config_dir));
    assert!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME)
            .is_some()
    );

    host.lock_keychain("User interaction is not allowed");
    host.forget_notes();
    super::place(
        &host,
        &store.config_dir,
        Some(STALE),
        None,
        super::IfItFails::TakeBack,
    )
    .expect_err("the copy behind the lock wins every read after it opens");
}

/// A store that refuses the removal and then says it holds nothing is a remark
/// rather than a refusal: the copy that would have won a read is not there.
///
/// Off macOS the file is the store read first, so it is the one a Credential in
/// the keychain beside it has to be cleared out of.
#[test]
fn a_superseded_copy_that_is_already_gone_is_noted_and_not_refused() {
    let host = logged_in_machine_off_macos().with_keychain_off_macos();
    let store = store_of(&host, EMAIL);
    super::make_dir(&host, &store.config_dir).expect("the Profile can be made");
    // Never written and refusing the removal anyway, which is the state a
    // directory somebody took the write bit off leaves.
    let host = host
        .with_a_path_refusing(
            &store.credentials_file,
            Refusing::Write,
            "Permission denied (os error 13)",
        )
        .with_a_path_refusing(
            &store.credentials_file,
            Refusing::Delete,
            "Permission denied (os error 13)",
        );

    super::store_credential(&host, &store, CREDENTIAL)
        .expect("the keychain took it and the file holds nothing to supersede it");

    assert_eq!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME)
            .as_deref(),
        Some(CREDENTIAL),
        "the Credential is where the write landed"
    );
    assert!(
        host.notes()
            .iter()
            .any(|note| note.contains("A superseded copy of a Credential could not be removed")),
        "and the machine says the store would not give a copy up: {:?}",
        host.notes()
    );
}
