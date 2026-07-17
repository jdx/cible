use std::path::Path;

use anyhow::Result;
use cible_core::Warehouse;
use serde_json::Value;

use crate::github;

pub fn run(repo: &str, prs: usize, db: &Path, force: bool, deep: bool) -> Result<()> {
    let wh = Warehouse::open(db)?;

    eprintln!("fetching last {prs} merged PRs for {repo}…");
    let pr_list = github::gh_json(&[
        "pr",
        "list",
        "--repo",
        repo,
        "--state",
        "merged",
        "--limit",
        &prs.to_string(),
        "--json",
        "number,title,mergedAt,headRefOid,files",
    ])?;

    let pr_list = pr_list.as_array().cloned().unwrap_or_default();
    let total = pr_list.len();
    let mut ingested = 0usize;
    let mut skipped = 0usize;

    for (i, pr) in pr_list.iter().enumerate() {
        let number = pr["number"].as_i64().unwrap_or(0);
        let already = wh.has_pr(repo, number)?;
        if !force && already && (!deep || wh.pr_is_deep(repo, number)?) {
            skipped += 1;
            continue;
        }
        let head_sha = pr["headRefOid"].as_str().unwrap_or_default();
        wh.upsert_pr(
            repo,
            number,
            pr["title"].as_str().unwrap_or_default(),
            pr["mergedAt"].as_str().unwrap_or_default(),
            head_sha,
        )?;
        for f in pr["files"].as_array().unwrap_or(&vec![]) {
            wh.upsert_pr_file(
                repo,
                number,
                f["path"].as_str().unwrap_or_default(),
                f["additions"].as_i64().unwrap_or(0),
                f["deletions"].as_i64().unwrap_or(0),
            )?;
        }
        let mut n_runs = ingest_runs_for_sha(&wh, repo, number, head_sha)?;
        if deep {
            // Merged PRs are green on their final commit almost by definition;
            // the failures that matter as replay ground truth happened on
            // earlier pushes.
            for sha in pr_commit_shas(repo, number)? {
                if sha != head_sha {
                    n_runs += ingest_runs_for_sha(&wh, repo, number, &sha)?;
                }
            }
            wh.mark_pr_deep(repo, number)?;
        }
        ingested += 1;
        eprintln!("[{}/{total}] PR #{number}: {n_runs} runs", i + 1);
    }

    let (n_prs, n_runs, n_jobs) = wh.counts(repo)?;
    println!(
        "done: ingested {ingested}, skipped {skipped} (already present); warehouse now has {n_prs} PRs, {n_runs} runs, {n_jobs} jobs"
    );
    Ok(())
}

fn pr_commit_shas(repo: &str, pr_number: i64) -> Result<Vec<String>> {
    let commits = github::gh_api_lines(
        &format!("repos/{repo}/pulls/{pr_number}/commits?per_page=100"),
        ".[].sha",
    )?;
    Ok(commits
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect())
}

fn ingest_runs_for_sha(wh: &Warehouse, repo: &str, pr_number: i64, sha: &str) -> Result<usize> {
    let runs = github::gh_api_lines(
        &format!("repos/{repo}/actions/runs?head_sha={sha}&per_page=100"),
        ".workflow_runs[]",
    )?;
    for run in &runs {
        let run_id = run["id"].as_i64().unwrap_or(0);
        wh.upsert_run(
            repo,
            run_id,
            Some(pr_number),
            run["name"].as_str().unwrap_or_default(),
            sha,
            run["event"].as_str().unwrap_or_default(),
            run["status"].as_str().unwrap_or_default(),
            run["conclusion"].as_str(),
            run["run_attempt"].as_i64().unwrap_or(1),
            run["created_at"].as_str().unwrap_or_default(),
            run["updated_at"].as_str().unwrap_or_default(),
        )?;
        ingest_jobs(wh, repo, run_id)?;
    }
    Ok(runs.len())
}

fn ingest_jobs(wh: &Warehouse, repo: &str, run_id: i64) -> Result<()> {
    // filter=all includes jobs from every attempt, which is what makes
    // rerun-based flake detection possible.
    let jobs: Vec<Value> = github::gh_api_lines(
        &format!("repos/{repo}/actions/runs/{run_id}/jobs?filter=all&per_page=100"),
        ".jobs[]",
    )?;
    for job in &jobs {
        wh.upsert_job(
            repo,
            job["id"].as_i64().unwrap_or(0),
            run_id,
            job["run_attempt"].as_i64().unwrap_or(1),
            job["name"].as_str().unwrap_or_default(),
            job["status"].as_str().unwrap_or_default(),
            job["conclusion"].as_str(),
            job["started_at"].as_str(),
            job["completed_at"].as_str(),
        )?;
    }
    Ok(())
}
