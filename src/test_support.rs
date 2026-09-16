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
