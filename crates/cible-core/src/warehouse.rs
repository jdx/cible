use std::path::Path;

use anyhow::Result;
use rusqlite::{Connection, params};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS prs (
  repo       TEXT NOT NULL,
  number     INTEGER NOT NULL,
  title      TEXT,
  merged_at  TEXT,
  head_sha   TEXT,
  PRIMARY KEY (repo, number)
);
CREATE TABLE IF NOT EXISTS pr_files (
  repo       TEXT NOT NULL,
  pr_number  INTEGER NOT NULL,
  path       TEXT NOT NULL,
  additions  INTEGER NOT NULL DEFAULT 0,
  deletions  INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (repo, pr_number, path)
);
CREATE TABLE IF NOT EXISTS runs (
  repo          TEXT NOT NULL,
  id            INTEGER NOT NULL,
  pr_number     INTEGER,
  workflow_name TEXT,
  head_sha      TEXT,
  event         TEXT,
  status        TEXT,
  conclusion    TEXT,
  run_attempt   INTEGER,
  created_at    TEXT,
  updated_at    TEXT,
  PRIMARY KEY (repo, id)
);
CREATE TABLE IF NOT EXISTS jobs (
  repo         TEXT NOT NULL,
  id           INTEGER NOT NULL,
  run_id       INTEGER NOT NULL,
  run_attempt  INTEGER,
  name         TEXT,
  status       TEXT,
  conclusion   TEXT,
  started_at   TEXT,
  completed_at TEXT,
  PRIMARY KEY (repo, id)
);
CREATE INDEX IF NOT EXISTS idx_jobs_run ON jobs (repo, run_id);
CREATE INDEX IF NOT EXISTS idx_runs_sha ON runs (repo, head_sha);
CREATE INDEX IF NOT EXISTS idx_runs_pr ON runs (repo, pr_number);
"#;

pub struct Warehouse {
    conn: Connection,
}

#[derive(Debug)]
pub struct FlakyJob {
    pub name: String,
    pub flakes: i64,
    pub wasted_minutes: f64,
}

#[derive(Debug)]
pub struct JobStat {
    pub name: String,
    pub runs: i64,
    pub avg_minutes: f64,
    pub total_minutes: f64,
    pub failure_rate: f64,
}

#[derive(Debug)]
pub struct PrCiStat {
    pub prs: i64,
    pub avg_wall_minutes: f64,
    pub p90_wall_minutes: f64,
}

impl Warehouse {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn has_pr(&self, repo: &str, number: i64) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM prs WHERE repo = ?1 AND number = ?2",
            params![repo, number],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn upsert_pr(
        &self,
        repo: &str,
        number: i64,
        title: &str,
        merged_at: &str,
        head_sha: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO prs (repo, number, title, merged_at, head_sha)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![repo, number, title, merged_at, head_sha],
        )?;
        Ok(())
    }

    pub fn upsert_pr_file(
        &self,
        repo: &str,
        pr_number: i64,
        path: &str,
        additions: i64,
        deletions: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO pr_files (repo, pr_number, path, additions, deletions)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![repo, pr_number, path, additions, deletions],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_run(
        &self,
        repo: &str,
        id: i64,
        pr_number: Option<i64>,
        workflow_name: &str,
        head_sha: &str,
        event: &str,
        status: &str,
        conclusion: Option<&str>,
        run_attempt: i64,
        created_at: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO runs
             (repo, id, pr_number, workflow_name, head_sha, event, status, conclusion, run_attempt, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                repo,
                id,
                pr_number,
                workflow_name,
                head_sha,
                event,
                status,
                conclusion,
                run_attempt,
                created_at,
                updated_at
            ],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_job(
        &self,
        repo: &str,
        id: i64,
        run_id: i64,
        run_attempt: i64,
        name: &str,
        status: &str,
        conclusion: Option<&str>,
        started_at: Option<&str>,
        completed_at: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO jobs
             (repo, id, run_id, run_attempt, name, status, conclusion, started_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![repo, id, run_id, run_attempt, name, status, conclusion, started_at, completed_at],
        )?;
        Ok(())
    }

    pub fn counts(&self, repo: &str) -> Result<(i64, i64, i64)> {
        let prs: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM prs WHERE repo = ?1",
            params![repo],
            |r| r.get(0),
        )?;
        let runs: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM runs WHERE repo = ?1",
            params![repo],
            |r| r.get(0),
        )?;
        let jobs: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM jobs WHERE repo = ?1",
            params![repo],
            |r| r.get(0),
        )?;
        Ok((prs, runs, jobs))
    }

    /// A flake: the same job name in the same run failed at one attempt and
    /// succeeded at a later attempt of the same commit.
    pub fn flaky_jobs(&self, repo: &str) -> Result<Vec<FlakyJob>> {
        let mut stmt = self.conn.prepare(
            "SELECT j1.name,
                    COUNT(*) AS flakes,
                    SUM((julianday(j1.completed_at) - julianday(j1.started_at)) * 1440.0) AS wasted
             FROM jobs j1
             JOIN jobs j2
               ON j2.repo = j1.repo AND j2.run_id = j1.run_id AND j2.name = j1.name
              AND j2.run_attempt > j1.run_attempt
             WHERE j1.repo = ?1
               AND j1.conclusion = 'failure'
               AND j2.conclusion = 'success'
             GROUP BY j1.name
             ORDER BY flakes DESC",
        )?;
        let rows = stmt.query_map(params![repo], |r| {
            Ok(FlakyJob {
                name: r.get(0)?,
                flakes: r.get(1)?,
                wasted_minutes: r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn job_stats(&self, repo: &str, limit: i64) -> Result<Vec<JobStat>> {
        let mut stmt = self.conn.prepare(
            "SELECT name,
                    COUNT(*) AS n,
                    AVG((julianday(completed_at) - julianday(started_at)) * 1440.0) AS avg_min,
                    SUM((julianday(completed_at) - julianday(started_at)) * 1440.0) AS total_min,
                    AVG(CASE WHEN conclusion = 'failure' THEN 1.0 ELSE 0.0 END) AS fail_rate
             FROM jobs
             WHERE repo = ?1 AND started_at IS NOT NULL AND completed_at IS NOT NULL
               AND conclusion IN ('success', 'failure')
             GROUP BY name
             ORDER BY total_min DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![repo, limit], |r| {
            Ok(JobStat {
                name: r.get(0)?,
                runs: r.get(1)?,
                avg_minutes: r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                total_minutes: r.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
                failure_rate: r.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Wall-clock CI time per PR: span from first run created to last run
    /// updated for that PR's head SHA.
    pub fn pr_ci_stats(&self, repo: &str) -> Result<PrCiStat> {
        let mut stmt = self.conn.prepare(
            "SELECT (julianday(MAX(r.updated_at)) - julianday(MIN(r.created_at))) * 1440.0 AS wall
             FROM prs p
             JOIN runs r ON r.repo = p.repo AND r.head_sha = p.head_sha
             WHERE p.repo = ?1
             GROUP BY p.number
             HAVING wall IS NOT NULL
             ORDER BY wall",
        )?;
        let walls: Vec<f64> = stmt
            .query_map(params![repo], |r| r.get::<_, f64>(0))?
            .collect::<std::result::Result<_, _>>()?;
        if walls.is_empty() {
            return Ok(PrCiStat { prs: 0, avg_wall_minutes: 0.0, p90_wall_minutes: 0.0 });
        }
        let avg = walls.iter().sum::<f64>() / walls.len() as f64;
        let p90 = walls[((walls.len() as f64 * 0.9) as usize).min(walls.len() - 1)];
        Ok(PrCiStat { prs: walls.len() as i64, avg_wall_minutes: avg, p90_wall_minutes: p90 })
    }

    /// Jobs that have never failed on their first attempt — candidates for
    /// providing zero information per run.
    pub fn never_failing_jobs(&self, repo: &str, min_runs: i64) -> Result<Vec<JobStat>> {
        let mut stmt = self.conn.prepare(
            "SELECT name,
                    COUNT(*) AS n,
                    AVG((julianday(completed_at) - julianday(started_at)) * 1440.0) AS avg_min,
                    SUM((julianday(completed_at) - julianday(started_at)) * 1440.0) AS total_min
             FROM jobs
             WHERE repo = ?1 AND started_at IS NOT NULL AND completed_at IS NOT NULL
               AND conclusion IN ('success', 'failure')
             GROUP BY name
             HAVING COUNT(*) >= ?2 AND SUM(CASE WHEN conclusion = 'failure' THEN 1 ELSE 0 END) = 0
             ORDER BY total_min DESC",
        )?;
        let rows = stmt.query_map(params![repo, min_runs], |r| {
            Ok(JobStat {
                name: r.get(0)?,
                runs: r.get(1)?,
                avg_minutes: r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                total_minutes: r.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
                failure_rate: 0.0,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
