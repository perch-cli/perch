#![allow(dead_code)]

use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

pub fn sessions_dir(profile: &Path) -> PathBuf {
    profile.join("sessions")
}

pub fn session_marker_at(profile: &Path, pid: u32) -> PathBuf {
    sessions_dir(profile).join(format!("{pid}.json"))
}

pub fn session_marker(pid: u32, at: DateTime<Utc>) -> String {
    serde_json::json!({"pid": pid, "startedAt": at.timestamp_millis(), "writtenBy": "perch"})
        .to_string()
}
