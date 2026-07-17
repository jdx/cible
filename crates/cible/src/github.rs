use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;

/// Run `gh` with the given args and parse stdout as JSON.
///
/// Shelling out to `gh` delegates auth, pagination, and rate-limit retries;
/// it is available locally and on GHA runners alike.
pub fn gh_json(args: &[&str]) -> Result<Value> {
    let out = Command::new("gh")
        .args(args)
        .output()
        .context("failed to spawn gh — is the GitHub CLI installed?")?;
    if !out.status.success() {
        bail!(
            "gh {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    serde_json::from_slice(&out.stdout).context("gh returned non-JSON output")
}

/// Run `gh api --paginate` with a `--jq` filter that emits one JSON value per
/// line, collecting all lines across pages.
pub fn gh_api_lines(endpoint: &str, jq: &str) -> Result<Vec<Value>> {
    let out = Command::new("gh")
        .args(["api", "--paginate", endpoint, "--jq", jq])
        .output()
        .context("failed to spawn gh")?;
    if !out.status.success() {
        bail!(
            "gh api {} failed: {}",
            endpoint,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).context("bad JSON line from gh api"))
        .collect()
}
