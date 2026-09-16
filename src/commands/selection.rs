//! Provider flags and installed-provider fallback belong to command policy.

use crate::providers::provider::{Id, Installation, catalog};
use crate::{Host, PerchError, Result};

#[derive(Debug, Default, Clone, Copy, clap::Args)]
pub struct Selection {
    /// Select any registered provider.
    #[arg(long, conflicts_with_all = ["claude", "codex"])]
    pub provider: Option<Id>,
    /// Select Claude Code explicitly.
    #[arg(long, conflicts_with = "codex")]
    pub claude: bool,
    /// Select Codex explicitly.
    #[arg(long, conflicts_with = "claude")]
    pub codex: bool,
}

impl Selection {
    pub fn explicit(self) -> Result<Option<Id>> {
        if let Some(provider) = self.provider {
            if self.claude || self.codex {
                return Err(PerchError::Invalid("Choose one provider selector".into()));
            }
            return Ok(Some(provider));
        }
        match (self.claude, self.codex) {
            (true, true) => Err(PerchError::Invalid(
                "choose only one of --claude and --codex".into(),
            )),
            (true, false) => Ok(Some(Id::Claude)),
            (false, true) => Ok(Some(Id::Codex)),
            _ => Ok(None),
        }
    }

    pub fn installed(self, host: &dyn Host, preferred: Id) -> Result<Installation> {
        if let Some(provider) = self.explicit()? {
            return provider.adapter().configured(host)?.installation(host);
        }
        for provider in std::iter::once(preferred).chain(
            catalog()
                .iter()
                .map(|adapter| adapter.id())
                .filter(|id| *id != preferred),
        ) {
            let configured = provider.adapter().configured(host)?;
            if !configured.enabled() {
                continue;
            }
            match configured.installation(host) {
                Ok(installation) => return Ok(installation),
                Err(PerchError::NotFound(_)) => {}
                Err(other) => return Err(other),
            }
        }
        Err(PerchError::NotFound("No enabled provider CLI is installed; configure a CLI path or enable a provider in config.json".into()))
    }
}
