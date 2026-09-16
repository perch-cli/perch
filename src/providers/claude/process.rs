//! Claude launch preparation and native shared state.

use super::{carry, reconcile};
use crate::providers::claude::probe;
use crate::providers::provider::{LaunchEnvironment, LaunchKind, LaunchRequest, PreparedLaunch};
use crate::{Host, Result};

pub(super) fn prepare<'a>(
    host: &'a dyn Host,
    request: &LaunchRequest<'_>,
) -> Result<PreparedLaunch<'a>> {
    let program = match request.kind {
        LaunchKind::Client(installed) => installed.executable().to_string_lossy().into_owned(),
        LaunchKind::Custom(program) => program.to_string(),
    };
    let profile = request.account.profile_dir(host)?;
    let default_profile = crate::providers::claude::layout::default_profile(host)?;
    let claim = crate::providers::sessions::claim(host, &profile)?;
    reconcile::reconcile(host, &default_profile.config_dir, &profile)?;
    carry::from_profiles(host, &request.shared_profiles, &profile);
    let path = probe::one_spelling(&profile);
    Ok(PreparedLaunch {
        program,
        arguments: request.arguments.to_vec(),
        environment: LaunchEnvironment::Overlay(vec![(
            "CLAUDE_CONFIG_DIR".into(),
            path.to_string_lossy().into_owned(),
        )]),
        _claim: Some(Box::new(claim)),
    })
}
