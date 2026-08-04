//! Fixtures for the behaviour tests: a machine with a Claude Code login on it.

use perch::commands::status::StatusArgs;
use perch::host::{Execution, FakeHost};

pub const CLAUDE_VERSION: &str = "2.1.221";
pub const LOGIN_NAME: &str = "someone";
pub const EMAIL: &str = "someone@example.com";
pub const DEFAULT_SERVICE: &str = "Claude Code-credentials";
pub const REGISTRY_PATH: &str = "/Users/someone/.perch/registry.json";

pub const CREDENTIAL: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-test","refreshToken":"sk-ant-ort01-test","expiresAt":1785000000000,"scopes":["user:inference","user:profile"],"subscriptionType":"pro"}}"#;

pub const IDENTITY_FILE: &str = r#"{
  "numStartups": 41,
  "oauthAccount": {
    "accountUuid": "account-uuid-1",
    "emailAddress": "someone@example.com",
    "organizationUuid": "organization-uuid-1",
    "organizationName": "Acme",
    "organizationRole": "admin"
  },
  "projects": {}
}"#;

/// A machine with Claude Code installed but nobody logged in.
pub fn machine_with_claude_code() -> FakeHost {
    FakeHost::new().with_exec(
        "claude",
        &["--version"],
        Execution {
            status: 0,
            stdout: format!("{CLAUDE_VERSION} (Claude Code)\n"),
            stderr: String::new(),
        },
    )
}

/// A machine with Claude Code installed and logged in — what everyone
/// installing Perch actually has.
pub fn logged_in_machine() -> FakeHost {
    machine_with_claude_code()
        .with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, CREDENTIAL)
        .with_file("/Users/someone/.claude.json", IDENTITY_FILE)
}

/// Runs `perch status` against a fake machine, returning what it printed
/// alongside how it ended.
pub fn run_status(host: &FakeHost, json: bool) -> (perch::Result<()>, String) {
    let mut written = Vec::new();
    let result = perch::commands::status::run(host, StatusArgs { json }, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
}
