//! A registry document from an older Perch, in the shape this build reads
//! (ADR a-registry-comes-forward).
//!
//! On the document rather than on typed values: the shapes this reads —
//! `Overrides`, `GlobalConfig`, `GroupConfig` — are gone from the tree, and
//! bringing them back as mirror types would be a second definition of a shape
//! nothing writes any more. What arrives is JSON and what leaves is JSON, so the
//! step is arithmetic and needs no machine to test.
//!
//! The registry migrates and an Export refuses (ADR the-holdings-outlive-a-perch).

use serde_json::{Map, Value};

use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::name::{self, NameKind};
use crate::registry::{Settings, UngroupedConfig};

/// The oldest version any published Perch stamped, and so the oldest shape this
/// has. Below it is a number no Perch wrote.
pub const EARLIEST_VERSION: u32 = 1;

/// Whether a document says nothing about its version: no `version` in it, or a
/// null one.
///
/// The one case worth a refusal of Perch's own. Any other kind of value there is
/// serde's to describe, and a file that is no document is the caller's.
pub fn says_no_version(text: &str) -> bool {
    serde_json::from_str::<Value>(text)
        .ok()
        .as_ref()
        .and_then(Value::as_object)
        .is_some_and(|held| held.get("version").unwrap_or(&Value::Null).is_null())
}

/// Whether a document claims a version below the oldest any Perch stamped, or
/// none at all.
///
/// The floor both readers of a registry hold. Neither number names a shape, and
/// reading one as the current shape is the half-parse a version exists to stop.
pub fn below_the_earliest(text: &str) -> bool {
    says_no_version(text)
        || crate::error::claimed_version(text)
            .is_some_and(|claimed| claimed < u64::from(EARLIEST_VERSION))
}

/// The registry document this build reads, or `None` where it already is one.
///
/// Nothing rather than an error over nonsense: what to make of that belongs to
/// the caller, which knows what file it is holding. Idempotent, because it is
/// reached twice — in memory on a read, and again on the run that writes it back.
pub fn forward(document: &str) -> Result<Option<String>> {
    // The version off a shape that is only the version, before the whole tree is
    // built: every command reaches this against a registry already current, and
    // a `Value` of every cached Utilization is a costly way to say "nothing".
    forward_from(document, crate::error::claimed_version(document))
}

/// The same where the caller has already read the version off the document.
///
/// Both readers of a registry have: `load` reads it to refuse a newer Perch, and
/// `behind` to say which version it is bringing forward. Reading it again here
/// is a second scan of the whole file to learn what the caller was holding.
pub fn forward_from(document: &str, claimed: Option<u64>) -> Result<Option<String>> {
    let Some(claimed) = claimed.filter(|at| a_step_moves_it(*at)) else {
        return Ok(None);
    };
    let Ok(Value::Object(held)) = serde_json::from_str::<Value>(document) else {
        return Ok(None);
    };

    let mut moved = match claimed == u64::from(EARLIEST_VERSION) {
        true => from_version_one(&held)?,
        false => held,
    };
    // Every step lands here rather than only the first: a name rule that gains
    // a member with nothing to carry the names already written down is the
    // refusal `load` turns into every command.
    rename_what_this_build_refuses(&mut moved, name::rules_for(claimed));
    moved.insert("version".to_string(), Value::from(CARRIED_TO));

    serde_json::to_string(&Value::Object(moved))
        .map(Some)
        .map_err(|err| PerchError::Other(format!("could not write the registry forward: {err}")))
}

/// The version 1 shape as version 2 held it: an `active` that is an object, the
/// Override layer folded into each Scope, and the retired Watcher Settings gone.
///
/// The version it lands on is stamped by the caller, which is the only place
/// that knows how many more steps are left after this one.
fn from_version_one(held: &Map<String, Value>) -> Result<Map<String, Value>> {
    let mut moved = held.clone();

    // Before any of the Scopes below are read out, so a Group renamed here is
    // renamed everywhere one is named: what it declares, what an Account claims,
    // and what a Check is keyed on.
    let renamed = renames_in(held, name::rules_for(u64::from(EARLIEST_VERSION)));

    // Read before Global goes, since both of the Scopes below are made out of it.
    let global = object("global", held.get("global"))?;
    let inherited = object("global.settings", global.get("settings"))?;

    match held.get("active") {
        Some(Value::String(email)) => {
            moved.insert("active".to_string(), settled(email));
        }
        // Nobody is `Active`'s default and is not written down, so an `active`
        // that named no Account leaves with the key gone rather than null.
        None | Some(Value::Null) => {
            moved.remove("active");
        }
        // A version 1 that already holds version 2's `Active` is a document
        // whose number belies its shape. Dropping it would lose which Account
        // the machine is on, silently.
        Some(_) => return Err(shape_belies_the_version("active")),
    }

    if let Some(accounts) = held.get("accounts").and_then(Value::as_array) {
        let carried = accounts
            .iter()
            .map(cycling_may_choose_it)
            .map(|account| Ok(claiming(account?, &renamed)))
            .collect::<Result<Vec<Value>>>()?;
        moved.insert("accounts".to_string(), Value::Array(carried));
    }

    let mut groups = Map::new();
    for (name, held) in object("groups", held.get("groups"))? {
        let said = object(&format!("groups.{name}"), Some(&held))?;
        groups.insert(
            now_called(NameKind::Group, &name, &renamed),
            settings(&inherited, &said),
        );
    }
    moved.insert("groups".to_string(), Value::Object(groups));

    let said = object("ungrouped", held.get("ungrouped"))?;
    moved.insert(
        "ungrouped".to_string(),
        ungrouped(&global, &inherited, &said),
    );
    moved.remove("global");

    let mut aliases = Map::new();
    for (name, email) in object("aliases", held.get("aliases"))? {
        aliases.insert(now_called(NameKind::Alias, &name, &renamed), email);
    }
    moved.insert("aliases".to_string(), Value::Object(aliases));

    let mut checks = Map::new();
    for (group, check) in object("checks", held.get("checks"))? {
        let mut kept = object(&format!("checks.{group}"), Some(&check))?;
        // The Account a Switch left was read by the no-return alone, and the
        // no-return could never fire (ADR a-watcher-knob-is-arithmetic).
        kept.remove("switched_off");
        // `ungrouped` is a legitimate key here and is not a Group, so a v1
        // registry that declared a Group by that name must not have the
        // Ungrouped Scope's Cooldown re-keyed onto the renamed Group.
        let under = match crate::name::means_ungrouped(&group) {
            // The constant rather than the spelling found: `record_switch` only
            // ever writes that one, so a key that folds to it under any other
            // capitalization becomes a second key `validate` refuses.
            true => crate::name::UNGROUPED.to_string(),
            false => now_called(NameKind::Group, &group, &renamed),
        };
        // Two v1 keys can land on one — a Group called `Ungrouped` beside the
        // Ungrouped Scope's own record — and the later Switch wins, as
        // `with_every_check_under_the_declared_spelling` settles the same one.
        let kept = Value::Object(kept);
        match checks.get(&under).and_then(switched_at) {
            Some(held) if switched_at(&kept).is_none_or(|arriving| arriving < held) => {}
            _ => {
                checks.insert(under, kept);
            }
        }
    }
    moved.insert("checks".to_string(), Value::Object(checks));

    Ok(moved)
}

/// Every name in a document of the shape this build reads brought inside this
/// build's name rules, in all four places one is written down.
///
/// On the current shape rather than version 1's, so a rule arriving after a
/// shape has shipped gets a step rather than an edit to the one below.
fn rename_what_this_build_refuses(held: &mut Map<String, Value>, wrote_it: &name::Rules) {
    let renamed = renames_in(held, wrote_it);
    if renamed.is_empty() {
        return;
    }

    if let Some(Value::Object(groups)) = held.get("groups") {
        let carried = groups
            .iter()
            .map(|(name, held)| (now_called(NameKind::Group, name, &renamed), held.clone()))
            .collect();
        held.insert("groups".to_string(), Value::Object(carried));
    }
    if let Some(Value::Object(aliases)) = held.get("aliases") {
        let carried = aliases
            .iter()
            .map(|(name, email)| (now_called(NameKind::Alias, name, &renamed), email.clone()))
            .collect();
        held.insert("aliases".to_string(), Value::Object(carried));
    }
    if let Some(Value::Array(accounts)) = held.get("accounts") {
        let carried = accounts
            .iter()
            .map(|account| claiming(account.clone(), &renamed))
            .collect();
        held.insert("accounts".to_string(), Value::Array(carried));
    }
    if let Some(Value::Object(checks)) = held.get("checks") {
        // `ungrouped` is a legitimate key here and is no Group, so it keeps its
        // spelling rather than being looked for among the renames.
        let carried = checks
            .iter()
            .map(|(group, check)| match crate::name::means_ungrouped(group) {
                true => (group.clone(), check.clone()),
                false => (now_called(NameKind::Group, group, &renamed), check.clone()),
            })
            .collect();
        held.insert("checks".to_string(), Value::Object(carried));
    }
}

/// What a name is called after the rename pass, which is itself where nothing
/// renamed it. Byte-exactly first, since two names that fold together get two
/// different new ones and a fold alone collapses them into one; folded behind
/// that, since an Account claiming `-DEV` of a Group declared `-dev` would
/// otherwise keep a claim naming the name this pass exists to take away.
fn now_called(kind: NameKind, name: &str, renamed: &[Renamed]) -> String {
    let of_the_kind = || renamed.iter().filter(|entry| entry.kind == kind);
    of_the_kind()
        .find(|entry| entry.was == name)
        .or_else(|| of_the_kind().find(|entry| crate::name::same_name(&entry.was, name)))
        .map_or_else(|| name.to_string(), |entry| entry.is_now.clone())
}

/// One Account with the Group it claims brought through the rename pass.
fn claiming(mut account: Value, renamed: &[Renamed]) -> Value {
    if let Some(held) = account.as_object_mut()
        && let Some(Value::String(claimed)) = held.get("group")
    {
        let now = now_called(NameKind::Group, claimed, renamed);
        held.insert("group".to_string(), Value::String(now));
    }
    account
}

/// A name a published Perch accepted that this build's rules refuse, said as it
/// was and as it is now.
///
/// One predicate per version, each version having shipped its own rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renamed {
    pub kind: NameKind,
    pub was: String,
    pub is_now: String,
}

/// What [`forward`] has to rename to bring a version 1 registry inside this
/// build's name rules.
///
/// Renamed rather than refused: `validate` is reached from `load`, so that
/// refusal takes every command with it — `perch group rename` among them.
pub fn renames(document: &str) -> Vec<Renamed> {
    // Nothing about a document no step moves, or this reports a rename [`forward`]
    // will never make: an Import says what was renamed before it writes.
    let Some(claimed) = crate::error::claimed_version(document).filter(|at| a_step_moves_it(*at))
    else {
        return Vec::new();
    };
    let carried = name::rules_for(claimed);
    serde_json::from_str::<Value>(document)
        .ok()
        .and_then(|held| held.as_object().map(|held| renames_in(held, carried)))
        .unwrap_or_default()
        .into_iter()
        // A name carried to itself is one the pass held on to, which is what
        // every name not mentioned here already says.
        .filter(|entry| entry.was != entry.is_now)
        .collect()
}

/// Whether [`forward`] has a step that moves a document claiming this version.
///
/// One answer, because [`renames`] says what that step will rename: two ranges
/// would be two answers to whether a document moves at all.
fn a_step_moves_it(claimed: u64) -> bool {
    (u64::from(EARLIEST_VERSION)..u64::from(CARRIED_TO)).contains(&claimed)
}

fn renames_in(held: &Map<String, Value>, wrote_it: &name::Rules) -> Vec<Renamed> {
    let names = |key: &str| -> Vec<String> {
        held.get(key)
            .and_then(Value::as_object)
            .map(|entries| entries.keys().cloned().collect())
            .unwrap_or_default()
    };
    let aliases = names("aliases");
    // A Group an Account claims is a Group name whether or not `groups` declares
    // it: `load` declares every claim it finds, so a claim left unrenamed is the
    // name `validate` refuses and no command is left to repair it with.
    let mut groups = names("groups");
    for claimed in claimed_groups(held) {
        // Folded, so a claim spelling a declared Group in another case is not a
        // second name.
        if !groups
            .iter()
            .any(|held| crate::name::same_name(held, &claimed))
        {
            groups.push(claimed);
        }
    }

    // One namespace, so a Group renamed out of the way of an Alias is a Group
    // that has not been renamed at all.
    let mut taken: Vec<String> = groups.clone();
    taken.extend(aliases.clone());

    let mut renamed = Vec::new();
    // What has been let stand, across both kinds because they share one
    // namespace. A name moves for what it is and for what it sits beside.
    let mut standing: Vec<(NameKind, String)> = Vec::new();
    for (kind, held) in [(NameKind::Group, groups), (NameKind::Alias, aliases)] {
        for was in held {
            let beside = standing
                .iter()
                .any(|(_, held)| told_them_apart(wrote_it, held, &was));
            if crate::name::current().accepts(&was) && !beside {
                standing.push((kind, was));
                continue;
            }
            // A name no Perch of this version ever accepted is a hand edit, and
            // is named at `load` rather than quietly given a different one.
            if !wrote_it.accepts(&was) {
                continue;
            }
            let Some(is_now) = name::acceptable(kind, &was, &taken) else {
                continue;
            };
            taken.push(is_now.clone());
            standing.push((kind, is_now.clone()));
            renamed.push(Renamed { kind, was, is_now });
        }
    }
    renamed.extend(kept_from_the_fold(&standing, &renamed));
    renamed
}

/// Whether two names one published Perch held as two are one to this build.
///
/// Two the older fold also held as one are a hand edit, which `load` names.
fn told_them_apart(wrote_it: &name::Rules, one: &str, other: &str) -> bool {
    name::same_name(one, other) && !wrote_it.one_name(one, other)
}

/// A name left standing said as a rename to itself, where one that folds
/// together with it moved.
///
/// [`now_called`]'s fold would otherwise carry the name that stayed to the new
/// name of the one that went, losing a Group. [`renames`] drops it from the report.
fn kept_from_the_fold(standing: &[(NameKind, String)], renamed: &[Renamed]) -> Vec<Renamed> {
    standing
        .iter()
        .filter(|(kind, name)| {
            renamed.iter().any(|entry| {
                entry.kind == *kind
                    && entry.was != *name
                    && crate::name::same_name(&entry.was, name)
            })
        })
        .map(|(kind, name)| Renamed {
            kind: *kind,
            was: name.clone(),
            is_now: name.clone(),
        })
        .collect()
}

/// When a `checks` record says its Switch happened. Parsed rather than compared
/// as text: chrono writes a fractional second only where there is one, and `.`
/// sorts below `Z`, so text order puts a record carrying one before a record of
/// the same instant without.
fn switched_at(check: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
    check
        .get("switched_at")
        .and_then(Value::as_str)
        .and_then(|at| at.parse().ok())
}

/// Every Group name an Account claims, in the order the Accounts are listed.
///
/// Duplicates and all: the caller folds them against the declared names, which
/// is the same question it asks of the declared ones themselves.
fn claimed_groups(held: &Map<String, Value>) -> Vec<String> {
    held.get("accounts")
        .and_then(Value::as_array)
        .map(|accounts| {
            accounts
                .iter()
                .filter_map(|account| account.get("group").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Brings the registry on this machine forward, once, before anything reads it.
///
/// Shape 1's sequence without shape 1's door (ADR one-door-to-the-registry),
/// which adopts a login where there is no registry. Reached from `main` because
/// every path that writes takes the lock before the read `load` would do it in.
pub fn bring_forward(host: &dyn Host) -> Result<()> {
    let path = crate::registry::registry_path(host)?;
    let Some(was) = behind(host, &path) else {
        return Ok(());
    };

    let mut perch = crate::registry::lock(host)?;
    // Asked again under the lock rather than trusted from outside it: between
    // the two reads, another Perch may have brought the same file forward.
    if behind(host, &path).is_none() {
        return Ok(());
    }
    let renamed = host
        .read_file(&path)
        .map(|held| renames(&held))
        .unwrap_or_default();
    let Some(mut registry) = crate::registry::load(host)? else {
        return Ok(());
    };
    // Through `save` rather than by writing what `forward` returned: it stamps
    // the version, refuses what a later `load` could not read, and replaces the
    // file in one step, so a migration that fails leaves the old shape intact.
    crate::registry::save(host, &mut perch, &mut registry)?;

    host.note(&format!(
        "This machine's registry was written by an older Perch (version {was}), \
         and has been brought forward to version {}.{} No older Perch reads the \
         file now.{}",
        crate::registry::CURRENT_VERSION,
        what_the_step_could_not_carry(was),
        what_was_renamed(&renamed),
    ));
    Ok(())
}

/// What a step left behind, for the note, or nothing where it left nothing.
///
/// The three retired Watcher Settings are the one thing any step drops, and
/// only the version 1 step reaches them: a version 2 document has none.
fn what_the_step_could_not_carry(was: u64) -> String {
    if was != u64::from(EARLIEST_VERSION) {
        return String::new();
    }
    " The Watcher's cooldown, margin and no-return are fixed in this build, so \
     whatever they were set to is gone; everything else came with it."
        .to_string()
}

/// What the step had to rename, for the note, or nothing where it renamed
/// nothing.
fn what_was_renamed(renamed: &[Renamed]) -> String {
    // Two newlines because this one is appended to a note that has already said
    // the version moved; the Import's stands alone.
    what_was_renamed_said(renamed).map_or_else(String::new, |said| format!("\n\n{said}"))
}

/// The same as a sentence of its own, for the other way a registry comes
/// forward: an Import, which says it before it writes rather than after.
pub fn what_was_renamed_said(renamed: &[Renamed]) -> Option<String> {
    if renamed.is_empty() {
        return None;
    }
    let said: Vec<String> = renamed
        .iter()
        .map(
            |entry| match crate::host::unshowable_character_in(&entry.was) {
                // A name refused for a character that draws as nothing draws
                // as one the registry may also hold: quoting `dev\u{FE00}` puts
                // a second `dev` beside the one the step did not touch.
                Some(said) => format!(
                    "{} carrying {said} is now `{}`",
                    entry.kind.article(),
                    entry.is_now
                ),
                None => format!(
                    "{} `{}` is now `{}`",
                    entry.kind.article(),
                    entry.was,
                    entry.is_now
                ),
            },
        )
        .collect();
    Some(format!(
        "This build refuses names it once accepted, so {} — nothing else about \
         {} changed.",
        said.join(", "),
        match renamed.len() {
            1 => "it",
            _ => "them",
        },
    ))
}

/// The version on disk, where [`forward`] has a step that moves it.
///
/// Asked of the step rather than of a range of its own: `save` stamps the current
/// version on whatever it is given, so a document nothing moved must not be one
/// this offers up — that would relabel a shape that never changed.
fn behind(host: &dyn Host, path: &std::path::Path) -> Option<u64> {
    let contents = host.read_file(path).ok()?;
    let was = crate::error::claimed_version(&contents)?;
    forward_from(&contents, Some(was))
        .ok()?
        .is_some()
        .then_some(was)
}

/// The version [`forward`] leaves behind, which is the one this build reads.
///
/// Written down rather than read off `CURRENT_VERSION`: a step that named the
/// current version would land on it whatever it became, so a shape moving with
/// no step to carry it would compile.
const CARRIED_TO: u32 = 4;

// Short of the shape this build reads, `load` deserializes what the step left
// behind as a shape it is not. Reading `CURRENT_VERSION` above would make this
// true by definition rather than by checking.
const _: () = assert!(CARRIED_TO == crate::registry::CURRENT_VERSION);

/// One Account, with the flag that inverted translated rather than dropped.
///
/// `enabled` defaulted to true where it was absent and `disabled` defaults to
/// false, so only the Account that said `false` has anything to say now — and
/// dropping the key instead would put a disabled Account back into Cycling.
fn cycling_may_choose_it(account: &Value) -> Result<Value> {
    let mut carried = object("an entry in `accounts`", Some(account))?;
    // Refused rather than read as `true`, which is `no_object_here`'s rule about
    // a map applied to the one field whose loss puts an Account somebody took
    // out of Cycling back into it.
    match carried.remove("enabled") {
        None | Some(Value::Bool(true)) => {}
        Some(Value::Bool(false)) => {
            carried.insert("disabled".to_string(), Value::Bool(true));
        }
        Some(_) => return Err(shape_belies_the_version("accounts[].enabled")),
    }
    Ok(Value::Object(carried))
}

/// What one Scope holds, out of what it said and what it Inherited.
///
/// Every Setting this build has, asked of the narrower first: reading what was
/// Inherited off the compiled-in defaults would revert a value somebody set. A
/// Setting this build lacks is never asked for, which is how the retired ones go.
fn settings(inherited: &Map<String, Value>, said: &Map<String, Value>) -> Value {
    let mut settings = own(serde_json::to_value(Settings::default()).unwrap_or_default());
    for (key, default) in settings.clone() {
        let value = said
            .get(&key)
            .or_else(|| inherited.get(&key))
            .cloned()
            .unwrap_or(default);
        settings.insert(key, value);
    }
    Value::Object(settings)
}

/// The Ungrouped Scope, which is Global's one Setting of its own beside the
/// Settings the Accounts in no Group said for themselves.
fn ungrouped(
    global: &Map<String, Value>,
    inherited: &Map<String, Value>,
    said: &Map<String, Value>,
) -> Value {
    let mut ungrouped = own(serde_json::to_value(UngroupedConfig::default()).unwrap_or_default());
    if let Some(declared) = global.get("cycle_ungrouped") {
        ungrouped.insert("interchangeable".to_string(), declared.clone());
    }
    ungrouped.insert("settings".to_string(), settings(inherited, said));
    Value::Object(ungrouped)
}

/// The Account the registry was left on, as `Active` spells it.
fn settled(email: &str) -> Value {
    let mut active = Map::new();
    active.insert("settled".to_string(), Value::from(email));
    Value::Object(active)
}

/// The refusal for a document holding one version's shape under another's
/// number.
///
/// Neither mechanism catches it — the file parses and what comes back is wrong —
/// so it is the one thing this step refuses rather than carries.
fn shape_belies_the_version(field: &str) -> PerchError {
    PerchError::Other(format!(
        "This registry says it is version {EARLIEST_VERSION}, and its `{field}` \
         is in a later shape. Perch will not guess which of the two the rest of \
         the file is in."
    ))
}

/// The refusal for a field this step reads as an object and this document holds
/// as something else.
///
/// Carrying it would mean carrying an empty one, which is the loss neither the
/// migration nor a later refusal can catch.
fn no_object_here(field: &str) -> PerchError {
    PerchError::Other(format!(
        "This registry says it is version {EARLIEST_VERSION}, and its `{field}` \
         is not the shape a version {EARLIEST_VERSION} registry wrote. Perch \
         will not read past it, because what it holds would be dropped rather \
         than refused."
    ))
}

/// Whatever object was there, and an empty one otherwise. A v1 registry left out
/// every key it had nothing to say about, so absent and empty are one case here
/// — and anything that is no object at all is the case [`no_object_here`] is for.
fn object(field: &str, value: Option<&Value>) -> Result<Map<String, Value>> {
    match value {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(held)) => Ok(held.clone()),
        Some(_) => Err(no_object_here(field)),
    }
}

/// A shape of Perch's own, as a map. Both callers serialize a struct, so there
/// is no value here a document could have chosen.
fn own(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A registry v0.2.0 wrote, serialized by v0.2.0's own serde impls.
    const V0_2_0: &str = include_str!("../tests/fixtures/registry-v0.2.0.json");

    /// A registry v0.1.1 wrote. The same `"version": 1` and a different shape:
    /// whole `GroupConfig` values under `groups`, no `ungrouped`, and a `global`
    /// holding nothing but the one flag.
    const V0_1_1: &str = include_str!("../tests/fixtures/registry-v0.1.1.json");

    fn forwarded(document: &str) -> Value {
        let moved = forward(document)
            .expect("a document a Perch wrote")
            .expect("this one is not the shape this build reads");
        serde_json::from_str(&moved).expect("what comes out is a document")
    }

    /// The floor the chain keeps: a number below the oldest any Perch stamped
    /// names no shape, so offering a step for it would stamp the current version
    /// on a document nothing ever wrote.
    #[test]
    fn a_version_no_perch_ever_stamped_is_offered_no_step() {
        for below in 0..EARLIEST_VERSION {
            let document = format!(r#"{{"version":{below},"accounts":[]}}"#);
            assert_eq!(
                forward(&document).expect("nothing is claimed about it"),
                None,
                "version {below} is below the earliest a Perch wrote"
            );
        }
    }

    /// `renames` says what the step will rename, and an Import prints that
    /// sentence before it writes — so a document no step moves has nothing to
    /// say, or the sentence describes a rename `forward` never makes.
    #[test]
    fn a_document_no_step_moves_names_no_rename() {
        let current = serde_json::json!({
            "version": crate::registry::CURRENT_VERSION,
            "accounts": [],
            "groups": { "dev★": {} },
        })
        .to_string();

        assert_eq!(forward(&current).expect("it is read"), None);
        assert_eq!(
            renames(&current),
            vec![],
            "a hand edit this build refuses is `load`'s to name, not the step's"
        );
    }

    #[test]
    fn a_document_this_build_already_reads_is_left_alone() {
        let current = format!(
            "{{\"version\":{},\"accounts\":[]}}",
            crate::registry::CURRENT_VERSION
        );
        assert_eq!(forward(&current).expect("it is read"), None);
    }

    #[test]
    fn nonsense_is_the_callers_to_refuse_rather_than_this_ones() {
        for not_a_registry in ["", "not json at all", "[]", "{}"] {
            assert_eq!(
                forward(not_a_registry).expect("nothing is claimed about it"),
                None,
                "the case this is about: {not_a_registry:?}"
            );
        }
    }

    /// The rules gained `global`, `ungrouped` and the leading `-` after version
    /// 1 shipped, and `validate` is reached from `load` — so leaving such a name
    /// as it is refuses every command, including the two that would repair it.
    #[test]
    fn a_name_a_published_perch_accepted_is_renamed_rather_than_refused() {
        let held: Value = serde_json::from_str(V0_2_0).expect("a document");
        let mut held = held.as_object().cloned().expect("an object");
        held.insert(
            "groups".to_string(),
            serde_json::json!({ "global": {}, "-dev": {}, "ungrouped": {} }),
        );
        held.insert(
            "aliases".to_string(),
            serde_json::json!({ "-w": "work@example.com" }),
        );
        held.insert("checks".to_string(), serde_json::json!({ "global": {} }));
        held.insert(
            "accounts".to_string(),
            serde_json::json!([{ "identity": { "email": "work@example.com" }, "group": "global" }]),
        );
        let document = Value::Object(held).to_string();

        let moved = forwarded(&document);
        assert_eq!(
            moved["groups"].as_object().map(|held| {
                let mut names: Vec<&String> = held.keys().collect();
                names.sort();
                names.into_iter().cloned().collect::<Vec<String>>()
            }),
            Some(vec![
                "dev".to_string(),
                "global-1".to_string(),
                "ungrouped-1".to_string()
            ]),
            "every declared Group comes forward under a name this build accepts"
        );
        assert_eq!(
            moved["accounts"][0]["group"], "global-1",
            "and the Account claiming it claims the name it is now called"
        );
        assert!(
            moved["checks"].get("global-1").is_some(),
            "as does the Check keyed on it, or the Cooldown paces nothing"
        );
        assert!(
            moved["aliases"].get("w").is_some(),
            "an Alias is the same namespace and the same rules"
        );
    }

    /// An escape is no `char::is_whitespace`, so every published Perch accepted
    /// a name holding one. A suffix rescues it no more than it rescues a leading
    /// `-`: the character has to go, or the machine has no working command.
    #[test]
    fn a_name_carrying_a_control_character_loses_it_rather_than_the_machine() {
        let renamed = renames(
            &serde_json::json!({
                "version": 1,
                "groups": { "\u{1b}[31mred": {}, "one\u{7}two": {} },
                "aliases": { "\u{7}w": "work@example.com", "wo\u{200b}rk": "spare@example.com" },
            })
            .to_string(),
        );

        let now: Vec<&str> = renamed.iter().map(|it| it.is_now.as_str()).collect();
        assert_eq!(
            now,
            ["31mred", "onetwo", "w", "work"],
            "the character goes and what a person meant by the name stays"
        );
        for name in now {
            assert!(
                crate::name::validate(NameKind::Group, name).is_ok(),
                "`{name}` is a name this build accepts"
            );
        }
    }

    /// Renaming into a name something already answers to would be no rename at
    /// all: Aliases and Group names share one namespace.
    #[test]
    fn a_rename_never_lands_on_a_name_already_taken() {
        let renamed = renames(
            &serde_json::json!({
                "version": 1,
                "groups": { "-dev": {}, "dev": {} },
                "aliases": { "dev-1": "work@example.com" },
            })
            .to_string(),
        );
        assert_eq!(
            renamed,
            vec![Renamed {
                kind: NameKind::Group,
                was: "-dev".to_string(),
                is_now: "dev-2".to_string(),
            }]
        );
    }

    /// A name that is nothing but the rule it breaks leaves no base to suffix,
    /// so the kind supplies one. `perch alias someone --` was a name v0.2.0
    /// accepted.
    #[test]
    fn a_name_with_nothing_left_after_the_rule_is_named_for_its_kind() {
        assert_eq!(
            renames(
                &serde_json::json!({
                    "version": 1,
                    "groups": { "--": {} },
                    "aliases": { "-": "work@example.com" },
                })
                .to_string()
            ),
            vec![
                Renamed {
                    kind: NameKind::Group,
                    was: "--".to_string(),
                    is_now: "group".to_string(),
                },
                Renamed {
                    kind: NameKind::Alias,
                    was: "-".to_string(),
                    is_now: "alias".to_string(),
                },
            ]
        );
    }

    /// v0.2.0 told two names apart by `to_lowercase`, and this build folds both
    /// spellings of a Greek sigma together — so a registry it wrote can hold two
    /// Groups this build reads as one name. The rename pass gives them two, and
    /// carrying the second one to the first one's new name would file both
    /// Groups' Accounts under one and lose the other Group's Settings outright.
    #[test]
    fn two_v1_names_this_build_folds_together_come_forward_as_two_groups() {
        let moved = forwarded(
            &serde_json::json!({
                "version": 1,
                "groups": { "-\u{3bf}\u{3b4}\u{3bf}\u{3c2}": { "watcher_threshold_percent": 90 },
                            "-\u{3bf}\u{3b4}\u{3bf}\u{3c3}": { "watcher_threshold_percent": 50 } },
                "accounts": [
                    { "identity": { "email": "a@b.com" }, "group": "-\u{3bf}\u{3b4}\u{3bf}\u{3c2}" },
                    { "identity": { "email": "c@d.com" }, "group": "-\u{3bf}\u{3b4}\u{3bf}\u{3c3}" },
                ],
            })
            .to_string(),
        );

        let groups = moved["groups"].as_object().expect("the Groups");
        assert_eq!(groups.len(), 2, "both Groups come forward: {groups:?}");
        assert_ne!(
            moved["accounts"][0]["group"], moved["accounts"][1]["group"],
            "and the two Accounts stay in the two Groups they were in"
        );
        for account in moved["accounts"].as_array().expect("the Accounts") {
            let claimed = account["group"].as_str().expect("a claim");
            assert!(
                groups.contains_key(claimed),
                "`{claimed}` is a Group that was declared"
            );
        }
    }

    /// v0.2.0 compared names with `to_lowercase`, which lowercases a final `Σ`
    /// to `ς`; this build folds `ς` and `σ` together. So it declared `ΧΡΟΝΟΣ`
    /// and `χρονοσ` as two Groups, and `validate` now reads them as one name and
    /// refuses — taking every command with it, `perch group rename` included.
    #[test]
    fn two_v1_names_this_build_folds_together_are_repaired_though_each_is_valid() {
        let upper = "\u{3a7}\u{3a1}\u{39f}\u{39d}\u{39f}\u{3a3}";
        let lower = "\u{3c7}\u{3c1}\u{3bf}\u{3bd}\u{3bf}\u{3c3}";
        assert!(
            crate::name::validate(NameKind::Group, upper).is_ok()
                && crate::name::validate(NameKind::Group, lower).is_ok(),
            "neither name is refused for itself, which is what hid this"
        );

        let moved = forwarded(
            &serde_json::json!({
                "version": 1,
                "groups": { upper: { "watcher_threshold_percent": 90 },
                            lower: { "watcher_threshold_percent": 50 } },
                "accounts": [
                    { "identity": { "email": "a@b.com" }, "group": upper },
                    { "identity": { "email": "c@d.com" }, "group": lower },
                ],
            })
            .to_string(),
        );

        let groups = moved["groups"].as_object().expect("the Groups");
        assert_eq!(groups.len(), 2, "both Groups come forward: {groups:?}");
        assert_ne!(
            moved["accounts"][0]["group"], moved["accounts"][1]["group"],
            "and the two Accounts stay in the two Groups they were in"
        );
        for account in moved["accounts"].as_array().expect("the Accounts") {
            let claimed = account["group"].as_str().expect("a claim");
            assert!(
                groups.contains_key(claimed),
                "`{claimed}` is a Group that was declared"
            );
        }
        assert_eq!(
            groups[upper]["watcher_threshold_percent"], 90,
            "the Group that kept its name kept its Settings: {groups:?}"
        );
    }

    /// Two names a published Perch could not have written as two — `to_lowercase`
    /// folded them as this build does — are a hand edit, and stay one for `load`
    /// to name rather than being given a second name here.
    #[test]
    fn two_names_no_published_perch_told_apart_are_left_for_the_refusal() {
        assert_eq!(
            renames(
                &serde_json::json!({ "version": 1, "groups": { "work": {}, "Work": {} } })
                    .to_string()
            ),
            Vec::new()
        );
    }

    /// The rename walks `accounts` to move the Group each one claims, and one
    /// that claims none comes through untouched rather than gaining a `group`.
    #[test]
    fn an_account_in_no_group_comes_through_claiming_none() {
        let moved = forwarded(
            &serde_json::json!({
                "version": 1,
                "groups": { "-dev": {} },
                "accounts": [{ "identity": { "email": "a@b.com" } }],
            })
            .to_string(),
        );
        assert_eq!(moved["accounts"].as_array().map(Vec::len), Some(1));
        assert!(moved["accounts"][0].get("group").is_none());
    }

    /// A name outside every rule — an `@`, which no published Perch accepted
    /// either — is a hand edit, and the refusal at `load` is what describes it.
    #[test]
    fn a_name_no_perch_ever_accepted_is_left_for_the_refusal() {
        assert_eq!(
            renames(&serde_json::json!({ "version": 1, "groups": { "a@b": {} } }).to_string()),
            vec![]
        );
    }

    #[test]
    fn the_active_account_becomes_the_settled_one() {
        assert_eq!(
            forwarded(V0_2_0)["active"],
            serde_json::json!({ "settled": "work@example.com" })
        );
    }

    /// The polarity inverts, so dropping the key rather than translating it
    /// would make a disabled Account live.
    #[test]
    fn an_account_kept_out_of_cycling_stays_out_of_it() {
        let accounts = forwarded(V0_2_0)["accounts"].clone();
        assert_eq!(accounts[0].get("disabled"), None, "one that was enabled");
        assert_eq!(
            accounts[1]["disabled"],
            Value::Bool(true),
            "and one that was not"
        );
        for account in accounts.as_array().expect("a list of Accounts") {
            assert_eq!(account.get("enabled"), None, "under either spelling");
        }
    }

    /// A Group that named two of its six Settings inherited the other four from
    /// Global, and Global is what the migration has to read them out of: taking
    /// the compiled-in defaults instead would silently revert a Setting somebody
    /// set.
    #[test]
    fn a_group_that_overrode_some_settings_keeps_the_ones_it_inherited() {
        assert_eq!(
            forwarded(V0_2_0)["groups"]["work"],
            serde_json::json!({
                "strategy": "most-headroom",
                "watcher_may_act": true,
                "watcher_threshold_percent": 80,
            })
        );
    }

    #[test]
    fn the_ungrouped_scope_is_global_and_what_the_ungrouped_accounts_said() {
        assert_eq!(
            forwarded(V0_2_0)["ungrouped"],
            serde_json::json!({
                "interchangeable": true,
                "settings": {
                    "strategy": "soonest-reset",
                    "watcher_may_act": false,
                    "watcher_threshold_percent": 85,
                },
            })
        );
    }

    #[test]
    fn a_scheduled_check_keeps_only_when_it_switched() {
        assert_eq!(
            forwarded(V0_2_0)["checks"]["work"],
            serde_json::json!({ "switched_at": "2026-08-14T10:00:00Z" })
        );
    }

    #[test]
    fn what_never_moved_arrives_as_it_was() {
        let moved = forwarded(V0_2_0);
        assert_eq!(moved["version"], Value::from(CARRIED_TO));
        assert_eq!(
            moved["aliases"],
            serde_json::json!({ "w": "work@example.com" })
        );
        assert_eq!(moved["accounts"][0]["plan"], Value::from("max"));
        assert_eq!(
            moved["accounts"][0]["utilization"]["windows"][0]["used_percent"],
            Value::from(42.0)
        );
        assert_eq!(moved.get("global"), None, "and Global is not among it");
    }

    /// The second shape `"version": 1` names. Its Groups are whole, so there is
    /// nothing to inherit; its `global` holds no Settings to inherit from; and
    /// the three retired watcher knobs are dropped rather than carried.
    #[test]
    fn the_shape_before_the_override_layer_comes_forward_too() {
        let moved = forwarded(V0_1_1);
        assert_eq!(
            moved["groups"]["work"],
            serde_json::json!({
                "strategy": "most-headroom",
                "watcher_may_act": true,
                "watcher_threshold_percent": 80,
            })
        );
        assert_eq!(
            moved["ungrouped"],
            serde_json::json!({
                "interchangeable": true,
                "settings": serde_json::to_value(Settings::default()).expect("the defaults"),
            }),
            "the Scope it never had, at the defaults it never said"
        );
        assert_eq!(moved["accounts"][1]["disabled"], Value::Bool(true));
    }

    /// Both documents, read as the types they are for rather than as JSON: the
    /// one assertion that the shape is *this build's* and not merely close to it.
    #[test]
    fn what_comes_forward_is_what_this_build_deserializes() {
        for (which, document) in [("v0.1.1", V0_1_1), ("v0.2.0", V0_2_0)] {
            let moved = forward(document)
                .expect("it comes forward")
                .expect("it is not current");
            let registry: crate::registry::Registry = serde_json::from_str(&moved)
                .unwrap_or_else(|err| panic!("a registry {which} wrote should deserialize: {err}"));
            assert_eq!(registry.version, crate::registry::CURRENT_VERSION);
            assert_eq!(registry.accounts.len(), 2, "{which}");
        }
    }

    /// Idempotence, which is what makes the step safe to reach twice: once in
    /// memory on a read and once again on the run that writes it back.
    #[test]
    fn a_document_that_came_forward_does_not_come_forward_again() {
        for document in [V0_1_1, V0_2_0] {
            let moved = forward(document)
                .expect("it comes forward")
                .expect("it is not current");
            assert_eq!(forward(&moved).expect("it is read"), None);
        }
    }

    /// The refusal is about a version, so it is owed only where the document had
    /// somewhere to say one and did not.
    #[test]
    fn a_document_that_says_no_version_is_told_from_one_serde_will_describe() {
        assert!(says_no_version("{}"));
        assert!(says_no_version(r#"{"accounts":[]}"#));
        assert!(says_no_version(r#"{"version":null}"#));
        assert!(!says_no_version(r#"{"version":2}"#));
        assert!(
            !says_no_version(r#"{"version":"2"}"#),
            "a version that is not a number is said in serde's words about the value"
        );
        for not_a_document in ["not json at all", "[]", ""] {
            assert!(!says_no_version(not_a_document), "{not_a_document:?}");
        }
    }

    /// Both halves of the floor, which the registry and an Export's registry
    /// share.
    #[test]
    fn a_version_below_the_earliest_and_none_at_all_are_one_answer() {
        assert!(below_the_earliest(r#"{"version":0,"accounts":[]}"#));
        assert!(below_the_earliest(r#"{"accounts":[]}"#));
        assert!(!below_the_earliest(r#"{"version":1,"accounts":[]}"#));
        assert!(!below_the_earliest(r#"{"version":2,"accounts":[]}"#));
    }

    /// A number that says one shape over a file holding another is the half-parse
    /// both mechanisms miss, so it is refused rather than carried.
    #[test]
    fn a_version_1_holding_version_2s_active_is_refused() {
        let belied = r#"{"version":1,"accounts":[],"active":{"settled":"someone@example.com"}}"#;

        let refused = forward(belied).expect_err("it says 1 and holds 2");

        assert!(refused.to_string().contains("later shape"), "{refused}");
    }

    #[test]
    fn an_active_that_names_nobody_is_carried_as_nobody() {
        for says_nobody in [
            r#"{"version":1,"accounts":[]}"#,
            r#"{"version":1,"accounts":[],"active":null}"#,
        ] {
            assert_eq!(
                forwarded(says_nobody).get("active"),
                None,
                "the case this is about: {says_nobody}"
            );
        }
    }

    /// Every field this step reads as an object, held as something else. Each
    /// was carried as an empty one before, which is a Setting reverted to a
    /// default or a Cooldown gone — under a note saying nothing was lost.
    #[test]
    fn a_field_that_is_no_object_is_refused_rather_than_carried_as_an_empty_one() {
        for (field, document) in [
            ("global", r#"{"version":1,"accounts":[],"global":[]}"#),
            (
                "global.settings",
                r#"{"version":1,"accounts":[],"global":{"settings":7}}"#,
            ),
            ("groups", r#"{"version":1,"accounts":[],"groups":[]}"#),
            (
                "groups.work",
                r#"{"version":1,"accounts":[],"groups":{"work":"watcher_may_act"}}"#,
            ),
            ("ungrouped", r#"{"version":1,"accounts":[],"ungrouped":""}"#),
            (
                "checks",
                r#"{"version":1,"accounts":[],"checks":[{"group":"work"}]}"#,
            ),
            (
                "checks.work",
                r#"{"version":1,"accounts":[],"checks":{"work":"2026-01-01T00:00:00Z"}}"#,
            ),
            (
                "an entry in `accounts`",
                r#"{"version":1,"accounts":["a"]}"#,
            ),
        ] {
            let refused = forward(document)
                .err()
                .unwrap_or_else(|| panic!("the case this is about: {document}"));
            let said = refused.to_string();
            assert!(said.contains(field), "{field} is the one to name: {said}");
        }
    }

    /// Every version below the one this build reads has a step that moves it.
    ///
    /// `forward` answers `None` for a version it does not recognize and `load`
    /// then reads that document as the current shape, which is a shape moving
    /// under a version that did not.
    #[test]
    fn every_version_short_of_the_current_one_is_carried_forward() {
        for version in EARLIEST_VERSION..crate::registry::CURRENT_VERSION {
            let document = format!(r#"{{"version":{version},"accounts":[]}}"#);

            let moved = forward(&document)
                .unwrap_or_else(|refused| panic!("version {version} is refused: {refused}"))
                .unwrap_or_else(|| {
                    panic!(
                        "version {version} is below the {} this build reads and \
                         no step moves it, so `load` reads it as the current shape",
                        crate::registry::CURRENT_VERSION
                    )
                });

            assert_eq!(
                crate::error::claimed_version(&moved),
                Some(u64::from(crate::registry::CURRENT_VERSION)),
                "and the step leaves it stamped with the shape it now holds"
            );
        }
    }

    /// `UngroupedConfig`'s own default is where `interchangeable` being off
    /// lives, so a v1 `global` that never said it is not a second place saying
    /// so.
    #[test]
    fn a_global_that_declared_nothing_leaves_the_ungrouped_scope_at_its_default() {
        let bare = r#"{"version":1,"accounts":[],"global":{}}"#;
        assert_eq!(
            forwarded(bare)["ungrouped"],
            serde_json::to_value(UngroupedConfig::default()).expect("the defaults")
        );
    }
}
