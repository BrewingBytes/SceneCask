//! The export worker: claims queued `export` jobs under a lease, builds the document from one
//! snapshot, stores it privately and marks the job ready for 24 hours. Each pass also settles
//! exhausted crashed claims, deletes expired artifacts before their rows, prunes old finished rows
//! and sweeps files no job records (a crash between writing a file and recording it).
//!
//! The claim count is the fencing token: a worker whose lease expired cannot record its result
//! over the claim that took over, and removes its own file instead. Logs carry only job IDs,
//! attempt counts and fixed failure categories, never paths or document content.

use std::{collections::HashSet, future::Future, time::Duration};

use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::{document, store::ExportStore};
use crate::{failure_kind, modules::catalog::refresh::Clock};

/// How long a claim is held. Building and writing one account's export fits well inside it; a
/// crashed worker's claim is recovered once it ends.
pub const LEASE: TimeDelta = TimeDelta::minutes(5);
/// Claims per job, including the first and any crashed ones. An exhausted job is marked failed
/// and the owner can request a new export.
pub const MAX_ATTEMPTS: i32 = 3;
/// How long a ready artifact can be downloaded (C08).
pub const TTL: TimeDelta = TimeDelta::hours(24);
/// Expired and failed rows are kept this long for status reads, then pruned.
const RETENTION: TimeDelta = TimeDelta::days(7);
/// Unrecorded files younger than this may belong to a claim that has not committed yet.
const ORPHAN_GRACE: Duration = Duration::from_secs(60 * 60);
const FIRST_RETRY: TimeDelta = TimeDelta::minutes(1);
const MAX_RETRY: TimeDelta = TimeDelta::minutes(30);
/// Expired artifacts deleted per pass.
const EXPIRE_BATCH: i64 = 100;

/// Why a claim did not produce an artifact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Reading the snapshot or recording the result failed; retried.
    Database,
    /// Writing the artifact failed; retried.
    Storage,
    /// The account is gone or disabled (deletion in progress); never retried.
    AccountUnavailable,
}

impl Failure {
    pub fn category(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::Storage => "storage",
            Self::AccountUnavailable => "account_unavailable",
        }
    }

    fn retryable(self) -> bool {
        !matches!(self, Self::AccountUnavailable)
    }
}

/// Outcome counts of one [`Exporter::run_once`] pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pass {
    pub exported: usize,
    pub retried: usize,
    pub failed: usize,
    /// Artifacts deleted because their 24 hours ended.
    pub expired: usize,
    /// Unrecorded files removed.
    pub swept: usize,
}

/// A claimed job.
#[derive(Clone, Copy, Debug)]
struct Job {
    id: Uuid,
    user_id: Uuid,
    /// Claims so far, including this one.
    attempts: i32,
}

enum Outcome {
    Exported,
    Retried,
    Failed,
    /// Another worker took over the expired lease; it owns the outcome.
    LeaseLost,
}

pub struct Exporter<C> {
    pool: PgPool,
    store: ExportStore,
    clock: C,
    /// Jobs claimed per pass, one at a time.
    batch: usize,
    poll: Duration,
}

impl<C: Clock> Exporter<C> {
    pub fn new(pool: PgPool, store: ExportStore, clock: C) -> Self {
        Self {
            pool,
            store,
            clock,
            batch: 4,
            poll: Duration::from_secs(10),
        }
    }

    /// Runs maintenance, then exports up to one batch of claimable jobs.
    pub async fn run_once(&self) -> Result<Pass, sqlx::Error> {
        let mut pass = self.maintain(self.clock.now()).await?;
        for _ in 0..self.batch {
            let Some(job) = self.claim(self.clock.now()).await? else {
                break;
            };
            match self.process(job).await? {
                Outcome::Exported => pass.exported += 1,
                Outcome::Retried => pass.retried += 1,
                Outcome::Failed => pass.failed += 1,
                Outcome::LeaseLost => {}
            }
        }
        Ok(pass)
    }

    /// Runs a pass at once and then after each poll interval until `shutdown` resolves. A pass
    /// that exported something runs again without waiting, so a queue drains promptly. Database
    /// failures are logged by category and retried after the interval.
    pub async fn run(self, shutdown: impl Future<Output = ()>) {
        tokio::pin!(shutdown);
        loop {
            let busy = match self.run_once().await {
                Ok(pass) => pass.exported + pass.retried + pass.failed == self.batch,
                Err(error) => {
                    tracing::warn!(category = failure_kind(&error), "export pass failed");
                    false
                }
            };
            let wait = if busy { Duration::ZERO } else { self.poll };
            tokio::select! {
                () = &mut shutdown => return,
                () = tokio::time::sleep(wait) => {}
            }
        }
    }

    async fn maintain(&self, now: DateTime<Utc>) -> Result<Pass, sqlx::Error> {
        sqlx::query(
            "UPDATE jobs SET state = 'failed', lease_until = NULL, updated_at = $1
             WHERE kind = 'export' AND state = 'running' AND lease_until <= $1
               AND attempts >= $2",
        )
        .bind(now)
        .bind(MAX_ATTEMPTS)
        .execute(&self.pool)
        .await?;
        let expired = self.expire(now).await?;
        // A row goes only once its file is gone, so no artifact outlives its record.
        sqlx::query(
            "DELETE FROM jobs WHERE kind = 'export' AND result_path IS NULL
               AND ((state = 'failed' AND updated_at < $1) OR (state = 'ready' AND expires_at < $1))",
        )
        .bind(now - RETENTION)
        .execute(&self.pool)
        .await?;
        let keep: HashSet<String> = sqlx::query_scalar(
            "SELECT result_path FROM jobs WHERE kind = 'export' AND result_path IS NOT NULL",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .collect();
        let swept = self
            .store
            .sweep(keep, ORPHAN_GRACE)
            .await
            .unwrap_or_else(|_| {
                tracing::warn!(category = "storage", "export sweep failed");
                0
            });
        Ok(Pass {
            expired,
            swept,
            ..Pass::default()
        })
    }

    /// Deletes the files of artifacts past their expiry, then forgets them. A file that cannot
    /// be deleted keeps its record and is tried again next pass.
    async fn expire(&self, now: DateTime<Utc>) -> Result<usize, sqlx::Error> {
        let due: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT id, result_path FROM jobs
             WHERE kind = 'export' AND state = 'ready' AND result_path IS NOT NULL
               AND expires_at <= $1
             ORDER BY expires_at LIMIT $2",
        )
        .bind(now)
        .bind(EXPIRE_BATCH)
        .fetch_all(&self.pool)
        .await?;
        let mut expired = 0;
        for (id, path) in due {
            if self.store.remove(&path).await.is_err() {
                tracing::warn!(job = %id, category = "storage", "export artifact removal failed");
                continue;
            }
            sqlx::query(
                "UPDATE jobs SET result_path = NULL, updated_at = $3
                 WHERE id = $1 AND result_path = $2",
            )
            .bind(id)
            .bind(&path)
            .bind(now)
            .execute(&self.pool)
            .await?;
            expired += 1;
        }
        Ok(expired)
    }

    /// Leases the oldest due job: a queued one whose retry time has passed or a running one whose
    /// lease ended (a crashed worker). Concurrent workers skip each other's rows.
    async fn claim(&self, now: DateTime<Utc>) -> Result<Option<Job>, sqlx::Error> {
        let row: Option<(Uuid, Option<Uuid>, i32)> = sqlx::query_as(
            "UPDATE jobs j
             SET state = 'running', attempts = j.attempts + 1, lease_until = $1, updated_at = $2
             FROM (
                 SELECT id FROM jobs
                 WHERE kind = 'export' AND attempts < $3
                   AND ((state = 'queued' AND (lease_until IS NULL OR lease_until <= $2))
                     OR (state = 'running' AND lease_until <= $2))
                 ORDER BY lease_until NULLS FIRST, created_at, id
                 LIMIT 1
                 FOR UPDATE SKIP LOCKED
             ) due
             WHERE j.id = due.id
             RETURNING j.id, j.user_id, j.attempts",
        )
        .bind(now + LEASE)
        .bind(now)
        .bind(MAX_ATTEMPTS)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|(id, user_id, attempts)| {
            // An export row always has its user (0006 CHECK); erasure deletes it first.
            Some(Job {
                id,
                user_id: user_id?,
                attempts,
            })
        }))
    }

    async fn process(&self, job: Job) -> Result<Outcome, sqlx::Error> {
        let document = match self.snapshot(job.user_id).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return self.settle_failure(job, Failure::AccountUnavailable).await,
            Err(_) => return self.settle_failure(job, Failure::Database).await,
        };
        let name = ExportStore::file_name(job.id, job.attempts);
        if self.store.write(name.clone(), document).await.is_err() {
            return self.settle_failure(job, Failure::Storage).await;
        }
        let now = self.clock.now();
        let recorded = sqlx::query(
            "UPDATE jobs SET state = 'ready', result_path = $3, expires_at = $4,
                 lease_until = NULL, updated_at = $5
             WHERE id = $1 AND attempts = $2 AND state = 'running'",
        )
        .bind(job.id)
        .bind(job.attempts)
        .bind(&name)
        .bind(now + TTL)
        .bind(now)
        .execute(&self.pool)
        .await;
        match recorded {
            Ok(done) if done.rows_affected() == 1 => Ok(Outcome::Exported),
            Ok(_) => {
                // Not recorded, so nobody can reach this file; the sweep retries a failed removal.
                let _ = self.store.remove(&name).await;
                Ok(Outcome::LeaseLost)
            }
            Err(_) => {
                let _ = self.store.remove(&name).await;
                self.settle_failure(job, Failure::Database).await
            }
        }
    }

    /// The serialized document for an enabled account, from one read-only snapshot; `None` when
    /// the account is gone or disabled.
    async fn snapshot(&self, user_id: Uuid) -> Result<Option<Vec<u8>>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let enabled: Option<bool> =
            sqlx::query_scalar("SELECT disabled_at IS NULL FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(&mut *tx)
                .await?;
        if enabled != Some(true) {
            return Ok(None);
        }
        let document = document::build(&mut tx, user_id, self.clock.now()).await?;
        tx.commit().await?;
        Ok(document.map(|document| {
            serde_json::to_vec(&document).expect("an export document always serializes")
        }))
    }

    /// Requeues a retryable failure after backoff, or marks the job failed.
    async fn settle_failure(&self, job: Job, failure: Failure) -> Result<Outcome, sqlx::Error> {
        let category = failure.category();
        let attempts = job.attempts;
        let retry = failure.retryable() && attempts < MAX_ATTEMPTS;
        if retry {
            tracing::warn!(job = %job.id, category, attempts, "export failed");
        } else {
            tracing::error!(job = %job.id, category, attempts, "export abandoned");
        }
        let now = self.clock.now();
        let (state, until, outcome) = if retry {
            ("queued", Some(now + backoff(attempts)), Outcome::Retried)
        } else {
            ("failed", None, Outcome::Failed)
        };
        let settled = sqlx::query(
            "UPDATE jobs SET state = $3, lease_until = $4, updated_at = $5
             WHERE id = $1 AND attempts = $2 AND state = 'running'",
        )
        .bind(job.id)
        .bind(attempts)
        .bind(state)
        .bind(until)
        .bind(now)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(if settled == 1 {
            outcome
        } else {
            Outcome::LeaseLost
        })
    }
}

/// Exponential backoff after the `attempts`-th failed claim: 1m, 2m, 4m, … capped at 30m.
fn backoff(attempts: i32) -> TimeDelta {
    let doublings = attempts.saturating_sub(1).clamp(0, 16) as u32;
    FIRST_RETRY
        .checked_mul(1 << doublings)
        .unwrap_or(MAX_RETRY)
        .min(MAX_RETRY)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    struct FixedClock(DateTime<Utc>);
    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    /// One queued export for a fresh account, and two workers over one store: `stale` at `t0`
    /// and `current` just after `stale`'s lease would end.
    async fn overlap(pool: PgPool) -> (Uuid, Exporter<FixedClock>, Exporter<FixedClock>, PathBuf) {
        let user: Uuid =
            sqlx::query_scalar("INSERT INTO users (normalized_email) VALUES ($1) RETURNING id")
                .bind(format!("{}@example.test", Uuid::new_v4().simple()))
                .fetch_one(&pool)
                .await
                .unwrap();
        let job: Uuid = sqlx::query_scalar(
            "INSERT INTO jobs (user_id, kind) VALUES ($1, 'export') RETURNING id",
        )
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
        let root = std::env::temp_dir().join(format!("scenecask-fence-{}", Uuid::new_v4()));
        let store = ExportStore::open(&root).unwrap();
        let t0 = Utc::now();
        let stale = Exporter::new(pool.clone(), store.clone(), FixedClock(t0));
        let current = Exporter::new(pool, store, FixedClock(t0 + LEASE + TimeDelta::seconds(1)));
        (job, stale, current, root)
    }

    async fn row(pool: &PgPool, job: Uuid) -> (String, i32, Option<String>) {
        sqlx::query_as("SELECT state, attempts, result_path FROM jobs WHERE id = $1")
            .bind(job)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[sqlx::test]
    async fn a_claim_that_lost_its_lease_cannot_record_its_artifact(pool: PgPool) {
        let (job, stale, current, root) = overlap(pool.clone()).await;
        let claimed = stale.claim(stale.clock.now()).await.unwrap().unwrap();
        assert_eq!((claimed.id, claimed.attempts), (job, 1));
        let took_over = current.claim(current.clock.now()).await.unwrap().unwrap();
        assert_eq!(took_over.attempts, 2);
        let files = || -> Vec<String> {
            std::fs::read_dir(&root)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect()
        };

        // The superseded claim finishes while the current one still runs: it writes its own file,
        // fails the fenced update and removes that file, recording nothing.
        assert!(matches!(
            stale.process(claimed).await.unwrap(),
            Outcome::LeaseLost
        ));
        assert_eq!(row(&pool, job).await, ("running".into(), 2, None));
        assert!(files().is_empty());

        assert!(matches!(
            current.process(took_over).await.unwrap(),
            Outcome::Exported
        ));
        let recorded = ExportStore::file_name(job, 2);
        assert_eq!(
            row(&pool, job).await,
            ("ready".into(), 2, Some(recorded.clone()))
        );
        assert_eq!(files(), [recorded]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[sqlx::test]
    async fn a_claim_that_lost_its_lease_cannot_settle_a_failure(pool: PgPool) {
        let (job, stale, current, root) = overlap(pool.clone()).await;
        let claimed = stale.claim(stale.clock.now()).await.unwrap().unwrap();
        let took_over = current.claim(current.clock.now()).await.unwrap().unwrap();
        assert_eq!(took_over.attempts, 2);
        for failure in [Failure::Storage, Failure::AccountUnavailable] {
            assert!(matches!(
                stale.settle_failure(claimed, failure).await.unwrap(),
                Outcome::LeaseLost
            ));
            assert_eq!(row(&pool, job).await, ("running".into(), 2, None));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backoff_doubles_and_caps() {
        for (attempts, minutes) in [(0, 1), (1, 1), (2, 2), (3, 4), (6, 30), (i32::MAX, 30)] {
            assert_eq!(backoff(attempts), TimeDelta::minutes(minutes), "{attempts}");
        }
    }

    #[test]
    fn only_an_unavailable_account_is_final() {
        assert!(Failure::Database.retryable() && Failure::Storage.retryable());
        assert!(!Failure::AccountUnavailable.retryable());
    }
}
