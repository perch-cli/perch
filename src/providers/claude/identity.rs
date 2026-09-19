//! Claude identity attribution and native identity fallback.

use crate::domain::Identity;

pub(super) fn subject(
    identity: &Identity,
) -> crate::Result<super::super::provider::AccountIdentity> {
    let user = identity.account_uuid.clone().ok_or_else(|| crate::PerchError::Invalid(
        "Claude did not supply a stable account UUID. Complete a fresh Claude login before adding this Account.".into()
    ))?;
    super::super::provider::AccountIdentity::from_subject(
        super::super::provider::Id::Claude,
        user,
        identity.organization_uuid.clone(),
    )
}

/// Stable subjects identify Accounts independently of their display email.
pub(super) fn names(identity: &Identity, account: &super::super::provider::ProfileRef) -> bool {
    match &account.provider_identity {
        Some(subject) => {
            identity.account_uuid.as_deref() == Some(subject.user_id.as_str())
                && identity.organization_uuid == subject.workspace_id
        }
        None => crate::name::same_name(&identity.email, account.email()),
    }
}

/// The `oauthAccount` block Claude Code would write for this Account, for
/// the Accounts whose Profile holds no identity file of its own.
pub(super) fn compose(identity: &Identity) -> String {
    let mut block = serde_json::Map::new();
    if let Some(uuid) = &identity.account_uuid {
        block.insert("accountUuid".into(), uuid.clone().into());
    }
    block.insert("emailAddress".into(), identity.email.clone().into());
    if let Some(organization) = &identity.organization_name {
        block.insert("organizationName".into(), organization.clone().into());
    }
    if let Some(uuid) = &identity.organization_uuid {
        block.insert("organizationUuid".into(), uuid.clone().into());
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(block))
        .expect("a map of strings serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_composed_block_carries_what_the_identity_knows_and_no_nulls() {
        let block = compose(&Identity {
            email: "someone@example.com".into(),
            account_uuid: Some("account-uuid-1".into()),
            organization_name: None,
            organization_uuid: None,
        });

        assert!(block.contains(r#""emailAddress": "someone@example.com""#));
        assert!(block.contains(r#""accountUuid": "account-uuid-1""#));
        assert!(!block.contains("organization"), "{block}");
    }

    #[test]
    fn a_composed_block_carries_the_organization_when_the_identity_has_one() {
        let block = compose(&Identity {
            email: "someone@example.com".into(),
            account_uuid: Some("account-uuid-1".into()),
            organization_name: Some("Example Ltd".into()),
            organization_uuid: Some("org-uuid-9".into()),
        });

        assert!(
            block.contains(r#""organizationName": "Example Ltd""#),
            "{block}"
        );
        assert!(
            block.contains(r#""organizationUuid": "org-uuid-9""#),
            "{block}"
        );
        assert!(
            block.contains(r#""emailAddress": "someone@example.com""#),
            "{block}"
        );
    }
}
