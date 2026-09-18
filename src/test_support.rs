//! Native fixture access for shared-module tests that arrange Claude credentials.

use crate::{Host, PerchError, Result};

pub(crate) trait AccountStoreFixture {
    fn store(&self, host: &dyn Host) -> Result<crate::claude_fixture::Store>;
}

impl AccountStoreFixture for crate::registry::Account {
    fn store(&self, host: &dyn Host) -> Result<crate::claude_fixture::Store> {
        if self.provider() != crate::providers::provider::Id::Claude {
            return Err(PerchError::Invalid(
                "This fixture requires a Claude Account".into(),
            ));
        }
        crate::claude_fixture::store_for_profile(host, &self.profile_dir(host)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Store this fixture derives is where Claude Code keeps a Credential,
    /// and a Profile of another provider holds nothing of the kind — so an
    /// Account from one is refused rather than given a path nothing writes.
    #[test]
    fn a_fixture_that_reaches_for_a_claude_store_refuses_an_account_of_another_provider() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let mut account = crate::cycle::tests::account("someone@example.com", vec![]);

        account.store(&host).expect("a Claude Account has one");

        account.provider = crate::providers::provider::Id::Codex;
        let Err(refused) = account.store(&host) else {
            panic!("and a Codex Account has not");
        };
        assert!(refused.to_string().contains("Claude Account"), "{refused}");
    }
}
