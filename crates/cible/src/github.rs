use std::process::{Command, Output};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

/// Run a gh command, retrying transient failures with backoff. GitHub's API
/// intermittently resets connections; a paginated multi-thousand-call ingest
/// must survive that.
fn run_with_retry(mut make: impl FnMut() -> Command, what: &str) -> Result<Output> {
    let mut last_err = String::new();
    for (i, delay) in [2u64, 5, 15].iter().enumerate() {
        let out = make().output().context("failed to spawn gh — is the GitHub CLI installed?")?;
        if out.status.success() {
            return Ok(out);
        }
        last_err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        eprintln!("  retry {}/3 for {what}: {last_err}", i + 1);
        std::thread::sleep(Duration::from_secs(*delay));
    }
    bail!("{what} failed after retries: {last_err}");
}

/// Run `gh` with the given args and parse stdout as JSON.
///
/// Shelling out to `gh` delegates auth, pagination, and rate-limit retries;
/// it is available locally and on GHA runners alike.
pub fn gh_json(args: &[&str]) -> Result<Value> {
    let out = run_with_retry(
        || {
            let mut c = Command::new("gh");
            c.args(args);
            c
        },
        &format!("gh {}", args.join(" ")),
    )?;
    serde_json::from_slice(&out.stdout).context("gh returned non-JSON output")
}

/// Run `gh api --paginate` with a `--jq` filter that emits one JSON value per
/// line, collecting all lines across pages.
pub fn gh_api_lines(endpoint: &str, jq: &str) -> Result<Vec<Value>> {
    let out = run_with_retry(
        || {
            let mut c = Command::new("gh");
            c.args(["api", "--paginate", endpoint, "--jq", jq]);
            c
        },
        &format!("gh api {endpoint}"),
    )?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).context("bad JSON line from gh api"))
        .collect()
}
