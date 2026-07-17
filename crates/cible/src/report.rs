use std::path::Path;

use anyhow::Result;
use cible_core::Warehouse;

pub fn run(repo: &str, db: &Path) -> Result<()> {
    let wh = Warehouse::open(db)?;
    let (prs, runs, jobs) = wh.counts(repo)?;
    println!("# CI report: {repo}");
    println!("warehouse: {prs} PRs, {runs} runs, {jobs} jobs\n");

    let ci = wh.pr_ci_stats(repo)?;
    println!("## PR CI wall time ({} PRs with runs)", ci.prs);
    println!("  avg {:.1} min · p90 {:.1} min\n", ci.avg_wall_minutes, ci.p90_wall_minutes);

    println!("## Flaky jobs (failed, then passed on rerun of same commit)");
    let flakes = wh.flaky_jobs(repo)?;
    if flakes.is_empty() {
        println!("  none detected");
    }
    for f in flakes.iter().take(15) {
        println!("  {:60} {:3} flakes · {:>7.1} min wasted", trunc(&f.name, 60), f.flakes, f.wasted_minutes);
    }

    println!("\n## Most expensive jobs (total minutes across ingested history)");
    for s in wh.job_stats(repo, 15)? {
        println!(
            "  {:60} {:5} runs · avg {:>5.1} min · total {:>8.0} min · {:>4.1}% fail",
            trunc(&s.name, 60),
            s.runs,
            s.avg_minutes,
            s.total_minutes,
            s.failure_rate * 100.0
        );
    }

    println!("\n## Jobs that never fail (≥50 runs, zero first-attempt failures)");
    let never = wh.never_failing_jobs(repo, 50)?;
    if never.is_empty() {
        println!("  none");
    }
    for s in never.iter().take(15) {
        println!(
            "  {:60} {:5} runs · total {:>8.0} min",
            trunc(&s.name, 60),
            s.runs,
            s.total_minutes
        );
    }

    Ok(())
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n - 1).collect();
        format!("{t}…")
    }
}
