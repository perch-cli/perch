//! Provider flags belong to command policy.

use crate::providers::provider::Id;

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
    /// The provider named on the command line, or none. clap refuses two at
    /// once, so a flag and `--provider` never both arrive.
    pub fn explicit(self) -> Option<Id> {
        self.provider.or(match (self.claude, self.codex) {
            (true, false) => Some(Id::Claude),
            (false, true) => Some(Id::Codex),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_names_the_flag_or_the_provider_or_nothing() {
        let named = |provider, claude, codex| {
            Selection {
                provider,
                claude,
                codex,
            }
            .explicit()
        };
        assert_eq!(named(None, false, false), None);
        assert_eq!(named(None, true, false), Some(Id::Claude));
        assert_eq!(named(None, false, true), Some(Id::Codex));
        assert_eq!(named(Some(Id::Codex), false, false), Some(Id::Codex));
    }
}
