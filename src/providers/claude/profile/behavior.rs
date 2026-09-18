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

/// An adoption may find no identity block to carry, and the Profile is made for
/// the Credential alone rather than for an Identity Perch invented.
#[test]
fn a_placement_carrying_no_identity_writes_the_credential_and_nothing_beside_it() {
    let host = FakeHost::new();
    let store = store_of(&host, EMAIL);

    super::place(
        &host,
        &store.config_dir,
        Some(CREDENTIAL),
        None,
        super::IfItFails::TakeBack,
    )
    .expect("a Credential is enough to make a Profile for");

    assert_eq!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME)
            .as_deref(),
        Some(CREDENTIAL)
    );
    assert_eq!(host.file(&store.identity_file), None);
}

/// The keychain namespace of a Profile is derived from the login name, so a
/// machine that will not say what that is has no Store to write into.
#[test]
fn a_placement_that_cannot_name_a_store_takes_back_the_directory_it_made() {
    let host = FakeHost::new().without_env("USER").without_env("USERNAME");
    let dir = std::path::Path::new("/profiles/someone-example-com");

    let refused = super::place(
        &host,
        dir,
        Some(CREDENTIAL),
        None,
        super::IfItFails::TakeBack,
    )
    .expect_err("no keychain account name can be derived");

    assert!(refused.to_string().contains("USER"), "{refused}");
    assert!(
        !host.path_exists(dir),
        "and the empty directory it made on the way is gone"
    );
}

#[test]
fn a_directory_that_will_not_go_after_that_is_named_beside_the_refusal() {
    let dir = "/profiles/someone-example-com";
    let host = FakeHost::new()
        .without_env("USER")
        .without_env("USERNAME")
        .with_a_path_refusing(dir, Refusing::Delete, "Permission denied (os error 13)");

    let refused = super::place(
        &host,
        std::path::Path::new(dir),
        Some(CREDENTIAL),
        None,
        super::IfItFails::TakeBack,
    )
    .expect_err("no keychain account name can be derived");

    let said = refused.to_string();
    assert!(said.contains("USER"), "{said}");
    assert!(said.contains("Rollback incomplete"), "{said}");
    assert!(said.contains(dir), "{said}");
}

/// The Profile is new and the write that would have justified it did not land,
/// so there is nothing for the policy to keep.
#[test]
fn a_placement_that_keeps_what_landed_still_takes_back_a_profile_nothing_landed_in() {
    let host = FakeHost::new();
    let store = store_of(&host, EMAIL);
    let host = host.with_a_path_refusing(
        &store.identity_file,
        Refusing::Write,
        "no space left on device",
    );

    let refused = super::place(
        &host,
        &store.config_dir,
        None,
        Some("{}"),
        super::IfItFails::KeepWhatLanded,
    )
    .expect_err("the Identity could not be written");

    assert!(refused.to_string().contains("no space left on device"));
    assert!(
        !host.path_exists(&store.config_dir),
        "a Profile holding nothing is nobody's"
    );
}

#[test]
fn a_placement_that_keeps_what_landed_leaves_the_credential_that_did() {
    let host = FakeHost::new();
    let store = store_of(&host, EMAIL);
    let host = host.with_a_path_refusing(
        &store.identity_file,
        Refusing::Write,
        "no space left on device",
    );

    let refused = super::place(
        &host,
        &store.config_dir,
        Some(CREDENTIAL),
        Some("{}"),
        super::IfItFails::KeepWhatLanded,
    )
    .expect_err("the Identity could not be written");

    assert!(refused.to_string().contains("no space left on device"));
    assert_eq!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME)
            .as_deref(),
        Some(CREDENTIAL),
        "the write went over whatever the Profile held before, so taking it \
         back would leave less than the caller started with"
    );
}

#[test]
fn a_placement_that_could_not_be_taken_back_says_that_beside_what_stopped_it() {
    let host = FakeHost::new();
    let store = store_of(&host, EMAIL);
    let host = host
        .with_a_path_refusing(
            &store.identity_file,
            Refusing::Write,
            "no space left on device",
        )
        .with_a_path_refusing(
            &store.credentials_file,
            Refusing::Delete,
            "Permission denied (os error 13)",
        );

    let refused = super::place(
        &host,
        &store.config_dir,
        Some(CREDENTIAL),
        Some("{}"),
        super::IfItFails::TakeBack,
    )
    .expect_err("the Identity could not be written");

    let said = refused.to_string();
    assert!(said.contains("no space left on device"), "{said}");
    assert!(said.contains("Rollback incomplete"), "{said}");
    assert!(said.contains("perch holdings purge"), "{said}");
}

/// The keychain is the store written on macOS, so the file beside it is where a
/// superseded copy would be left.
#[test]
fn a_superseded_copy_the_second_store_will_not_give_up_is_noted_and_not_refused() {
    let host = FakeHost::new();
    let store = store_of(&host, EMAIL);
    let host = host.with_a_path_refusing(
        &store.credentials_file,
        Refusing::Delete,
        "Permission denied (os error 13)",
    );
    super::make_dir(&host, &store.config_dir).expect("the Profile can be made");

    super::store_credential(&host, &store, CREDENTIAL).expect("the keychain took it");

    let notes = host.notes();
    assert!(
        notes
            .iter()
            .any(|note| note.contains("A superseded copy of a Credential could not be removed")),
        "{notes:?}"
    );
}

/// Off macOS the file is written first, so a file that takes the bytes and then
/// will not be read is the store whose answer is missing.
#[test]
fn a_store_that_takes_a_credential_and_then_says_nothing_is_left_to_the_other_one() {
    let host = logged_in_machine_off_macos().with_keychain_off_macos();
    let store = store_of(&host, EMAIL);
    super::make_dir(&host, &store.config_dir).expect("the Profile can be made");
    let host = host.with_a_path_refusing(
        &store.credentials_file,
        Refusing::Read,
        "Permission denied (os error 13)",
    );

    super::store_credential(&host, &store, CREDENTIAL).expect("the keychain took it instead");

    assert_eq!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME)
            .as_deref(),
        Some(CREDENTIAL),
        "the Credential is in the store that answered"
    );
    let notes = host.notes();
    assert!(
        notes
            .iter()
            .any(|note| note.contains("could not be written to")),
        "{notes:?}"
    );
}

/// Both stores take the Credential and hand back something else: the file is
/// arranged to corrupt what it is given, and the keychain to keep four bytes of
/// it, which is `security -i` overrunning its stdin buffer.
#[test]
fn a_bad_copy_that_cannot_be_taken_out_is_said_rather_than_left_unmentioned() {
    let host = logged_in_machine_off_macos().with_keychain_off_macos();
    let store = store_of(&host, EMAIL);
    super::make_dir(&host, &store.config_dir).expect("the Profile can be made");
    let host = host
        .with_file_corrupting_writes(&store.credentials_file)
        .with_a_path_refusing(
            &store.credentials_file,
            Refusing::Delete,
            "Permission denied (os error 13)",
        )
        .with_keychain_truncating_after(4);
    host.forget_notes();

    let refused = super::store_credential(&host, &store, CREDENTIAL)
        .expect_err("neither store holds what it was handed");

    assert!(
        refused.to_string().contains("did not read back intact"),
        "the store this machine reads first is the one reported: {refused}"
    );
    let notes = host.notes();
    assert!(
        notes
            .iter()
            .any(|note| note.contains("That copy could not be removed from")),
        "{notes:?}"
    );
    assert_eq!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME),
        None,
        "and the copy that could be taken out was"
    );
}
