use std::path::Path;

use anyhow::Result;
use cible_core::Warehouse;

/// List genuine (flake-filtered) failures on PR commits — the ground truth
/// that replay scoring runs against.
pub fn run(repo: &str, db: &Path) -> Result<()> {
    let wh = Warehouse::open(db)?;
    let failures = wh.real_failures(repo)?;

    let mut prs = std::collections::BTreeSet::new();
    for f in &failures {
        prs.insert(f.pr_number);
        println!(
            "PR #{:<6} {}  {} / {}",
            f.pr_number,
            f.head_sha.get(..12).unwrap_or(&f.head_sha),
            f.workflow_name,
            f.job_name
        );
    }
    println!(
        "\n{} real failures across {} PRs (flake-filtered)",
        failures.len(),
        prs.len()
    );
    Ok(())
}
