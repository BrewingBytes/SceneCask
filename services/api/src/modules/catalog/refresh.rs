//! R12 metadata refresh (C02, C05). Every 24 hours each saved show is re-fetched through a leased
//! `metadata_refresh` job in `jobs` (0006). A refresh applies the whole snapshot atomically with
//! the importer's rules: episodes match by provider identity through renumbering, removed rows
//! are archived, progress is never written or deleted, and a collision rolls back to the last
//! good catalog. Catalog triggers advance `catalog_revision` only when membership, order or
//! release data change. Failed refreshes retry with jittered backoff; a provider 429 pauses the
//! worker for its Retry-After. A show not refreshed for 48 hours is disclosed as stale.
//!
//! Logs carry only job IDs, attempt counts and fixed failure categories: never titles, image
//! paths, provider payloads or database error text.

use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use chrono::{DateTime, TimeDelta, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::{
    import::{begin_locked, fetch, import_database_error, persist},
    provider::{Catalog, ProviderError, TvProvider},
};
use crate::{
    error::{ApiError, ErrorCode},
    failure_kind,
};

/// How often a saved show is refreshed.
pub const REFRESH_INTERVAL: TimeDelta = TimeDelta::hours(24);
/// A show whose last successful fetch is older than this is reported as `metadataStale`.
pub const STALE_AFTER: TimeDelta = TimeDelta::hours(48);
/// A claim outlives the 30-second catalog fetch budget plus the bounded write transaction, so a
/// live refresh is not reclaimed; a crashed worker's claim is recovered once it ends.
pub const LEASE: TimeDelta = TimeDelta::minutes(2);
/// Claims per job, including the first and any crashed ones. An exhausted job is marked failed;
/// the show is scheduled again after the next refresh interval.
pub const MAX_ATTEMPTS: i32 = 6;
/// Finished jobs are kept this long for operations, then pruned.
const RETENTION: TimeDelta = TimeDelta::days(7);
const FIRST_RETRY: TimeDelta = TimeDelta::minutes(1);
const MAX_RETRY: TimeDelta = TimeDelta::hours(1);

/// Whether a show last fetched at `fetched_at` is stale at `now` (C05 `metadataStale`). The
/// show's saved catalog and every user's history stay usable either way.
pub fn metadata_stale(fetched_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now - fetched_at > STALE_AFTER
}

/// Server time source; tests inject a manual clock.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        SystemTime::now().into()
    }
}
impl<C: Clock + ?Sized> Clock for Arc<C> {
    fn now(&self) -> DateTime<Utc> {
        (**self).now()
    }
}

/// Outcome counts of one [`Refresher::run_once`] pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pass {
    pub scheduled: u64,
    pub refreshed: usize,
    pub retried: usize,
    pub rate_limited: usize,
    pub failed: usize,
    /// The provider's cooldown was still running, so nothing was claimed.
    pub paused: bool,
}

/// A claimed job.
#[derive(Clone, Copy, Debug)]
struct Job {
    id: Uuid,
    show_id: Option<Uuid>,
    /// Claims so far, including this one. A claim taken over after its lease ended has moved on.
    attempts: i32,
}

enum Outcome {
    Refreshed,
    Retried,
    RateLimited,
    Failed,
    /// Another worker took over the expired lease; it owns the outcome.
    LeaseLost,
}

pub struct Refresher<P, C> {
    pool: PgPool,
    provider: Arc<P>,
    clock: C,
    /// Jobs refreshed concurrently per pass.
    concurrency: i64,
    /// Shows scheduled per pass, stalest first.
    batch: i64,
    poll: Duration,
    paused_until: Mutex<Option<DateTime<Utc>>>,
}

impl<P: TvProvider + 'static, C: Clock + 'static> Refresher<P, C> {
    pub fn new(pool: PgPool, provider: Arc<P>, clock: C) -> Self {
        Self {
            pool,
            provider,
            clock,
            concurrency: 4,
            batch: 100,
            poll: Duration::from_secs(60),
            paused_until: Mutex::new(None),
        }
    }

    /// Schedules due shows, then refreshes up to one bounded batch of claimable jobs.
    pub async fn run_once(self: &Arc<Self>) -> Result<Pass, sqlx::Error> {
        let now = self.clock.now();
        let mut pass = Pass {
            scheduled: self.schedule(now).await?,
            ..Pass::default()
        };
        if self.paused(now) {
            pass.paused = true;
            return Ok(pass);
        }
        let mut tasks = tokio::task::JoinSet::new();
        for job in self.claim(now).await? {
            let this = Arc::clone(self);
            tasks.spawn(async move { this.process(job).await });
        }
        while let Some(result) = tasks.join_next().await {
            let outcome = match result {
                Ok(outcome) => outcome?,
                // A panicked refresh leaves its claim to lease recovery.
                Err(_) => continue,
            };
            match outcome {
                Outcome::Refreshed => pass.refreshed += 1,
                Outcome::Retried => pass.retried += 1,
                Outcome::RateLimited => pass.rate_limited += 1,
                Outcome::Failed => pass.failed += 1,
                Outcome::LeaseLost => {}
            }
        }
        Ok(pass)
    }

    /// Runs a pass every poll interval, starting at once, until `shutdown` resolves. Database
    /// failures are logged by category and retried on the next tick.
    pub async fn run(self: Arc<Self>, shutdown: impl Future<Output = ()>) {
        let mut ticks = tokio::time::interval(self.poll);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tokio::pin!(shutdown);
        while tokio::select! {
            () = &mut shutdown => false,
            _ = ticks.tick() => true,
        } {
            if let Err(error) = self.run_once().await {
                let category = failure_kind(&error);
                tracing::warn!(category, "metadata refresh pass failed");
            }
        }
    }

    fn paused(&self, now: DateTime<Utc>) -> bool {
        let paused = self.paused_until.lock().unwrap_or_else(|e| e.into_inner());
        paused.is_some_and(|until| until > now)
    }

    fn pause(&self, until: DateTime<Utc>) {
        let mut paused = self.paused_until.lock().unwrap_or_else(|e| e.into_inner());
        if paused.is_none_or(|current| current < until) {
            *paused = Some(until);
        }
    }

    /// Queues one job per saved show not fetched within the refresh interval, unless the show
    /// has an active job or one created within the interval (so an exhausted job waits a full
    /// interval rather than being requeued at once). Also settles exhausted crashed claims and
    /// prunes old finished jobs.
    async fn schedule(&self, now: DateTime<Utc>) -> Result<u64, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        // Serializes concurrent schedulers; `jobs` has no per-show uniqueness for this kind.
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended('catalog.refresh.schedule', 0))",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE jobs SET state = 'failed', lease_until = NULL, updated_at = $1
             WHERE kind = 'metadata_refresh' AND state = 'running' AND lease_until <= $1
               AND attempts >= $2",
        )
        .bind(now)
        .bind(MAX_ATTEMPTS)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM jobs WHERE kind = 'metadata_refresh' AND state IN ('ready', 'failed')
               AND updated_at < $1",
        )
        .bind(now - RETENTION)
        .execute(&mut *tx)
        .await?;
        let scheduled = sqlx::query(
            "INSERT INTO jobs (kind, payload, created_at, updated_at)
             SELECT 'metadata_refresh', jsonb_build_object('showId', s.id), $1, $1
             FROM (
                 SELECT s.id FROM shows s
                 WHERE s.fetched_at <= $2
                   AND EXISTS (SELECT FROM library_entries l WHERE l.show_id = s.id AND l.saved)
                   AND NOT EXISTS (
                       SELECT FROM jobs j
                       WHERE j.kind = 'metadata_refresh' AND j.payload ->> 'showId' = s.id::text
                         AND (j.state IN ('queued', 'running') OR j.created_at > $2))
                 ORDER BY s.fetched_at, s.id
                 LIMIT $3
             ) s",
        )
        .bind(now)
        .bind(now - REFRESH_INTERVAL)
        .bind(self.batch)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(scheduled)
    }

    /// Leases due jobs: queued ones whose retry time has passed and running ones whose lease
    /// ended (a crashed worker). Concurrent workers skip each other's rows.
    async fn claim(&self, now: DateTime<Utc>) -> Result<Vec<Job>, sqlx::Error> {
        let rows = sqlx::query(
            "UPDATE jobs j
             SET state = 'running', attempts = j.attempts + 1, lease_until = $1, updated_at = $2
             FROM (
                 SELECT id FROM jobs
                 WHERE kind = 'metadata_refresh' AND attempts < $3
                   AND ((state = 'queued' AND (lease_until IS NULL OR lease_until <= $2))
                     OR (state = 'running' AND lease_until <= $2))
                 ORDER BY lease_until NULLS FIRST, created_at, id
                 LIMIT $4
                 FOR UPDATE SKIP LOCKED
             ) due
             WHERE j.id = due.id
             RETURNING j.id, j.payload ->> 'showId' AS show_id, j.attempts",
        )
        .bind(now + LEASE)
        .bind(now)
        .bind(MAX_ATTEMPTS)
        .bind(self.concurrency)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(Job {
                    id: row.try_get("id")?,
                    show_id: row
                        .try_get::<Option<String>, _>("show_id")?
                        .and_then(|id| id.parse().ok()),
                    attempts: row.try_get("attempts")?,
                })
            })
            .collect()
    }

    async fn process(&self, job: Job) -> Result<Outcome, sqlx::Error> {
        let show = match job.show_id {
            Some(id) => {
                sqlx::query_scalar::<_, i64>("SELECT tmdb_id FROM shows WHERE id = $1")
                    .bind(id)
                    .fetch_optional(&self.pool)
                    .await?
            }
            None => None,
        };
        let (Some(show_id), Some(provider_id)) = (job.show_id, show) else {
            tracing::error!(job = %job.id, "metadata refresh job names no show");
            return self.settle(job, None, Outcome::Failed).await;
        };
        // Do not hold a database transaction while waiting on provider I/O.
        let catalog = match fetch(&*self.provider, provider_id).await {
            Ok(catalog) => catalog,
            Err(ProviderError::RateLimited(delay)) => {
                let until = self.clock.now() + cooldown(delay);
                self.pause(until);
                return self.retry(job, "rate_limited", Some(until)).await;
            }
            Err(ProviderError::Unavailable) => {
                return self.retry(job, "provider_unavailable", None).await;
            }
            Err(ProviderError::InvalidData) => {
                return self.retry(job, "invalid_snapshot", None).await;
            }
        };
        match self.apply(job, show_id, provider_id, &catalog).await {
            Ok(true) => Ok(Outcome::Refreshed),
            Ok(false) => Ok(Outcome::LeaseLost),
            Err(error) if error.code() == ErrorCode::ProviderUnavailable => {
                self.retry(job, "invalid_snapshot", None).await
            }
            Err(_) => self.retry(job, "database", None).await,
        }
    }

    /// Applies the snapshot and settles the job in one transaction, so a refresh commits only
    /// while its claim is still held. Returns false, writing nothing, if the claim was lost.
    async fn apply(
        &self,
        job: Job,
        show_id: Uuid,
        provider_id: i64,
        catalog: &Catalog,
    ) -> Result<bool, ApiError> {
        let mut tx = begin_locked(&self.pool, provider_id).await?;
        let now = self.clock.now();
        // An import may have refreshed the show since this job was queued.
        let due: bool =
            sqlx::query_scalar("SELECT fetched_at <= $2 FROM shows WHERE id = $1 FOR UPDATE")
                .bind(show_id)
                .bind(now - REFRESH_INTERVAL)
                .fetch_one(&mut *tx)
                .await?;
        if due {
            persist(&mut tx, show_id, catalog, now).await?;
        }
        if !settle_in(&mut tx, job, None, "ready", now).await? {
            tx.rollback().await?;
            return Ok(false);
        }
        tx.commit().await.map_err(import_database_error)?;
        Ok(true)
    }

    /// Requeues the claim after jittered backoff (or the provider's cooldown `until`), or fails
    /// an exhausted job. The last good catalog is untouched either way.
    async fn retry(
        &self,
        job: Job,
        category: &'static str,
        until: Option<DateTime<Utc>>,
    ) -> Result<Outcome, sqlx::Error> {
        let attempts = job.attempts;
        if attempts >= MAX_ATTEMPTS {
            tracing::error!(job = %job.id, category, attempts, "metadata refresh exhausted its retries");
            return self.settle(job, None, Outcome::Failed).await;
        }
        tracing::warn!(job = %job.id, category, attempts, "metadata refresh failed");
        let until = until.unwrap_or_else(|| self.clock.now() + backoff(attempts));
        let outcome = if category == "rate_limited" {
            Outcome::RateLimited
        } else {
            Outcome::Retried
        };
        self.settle(job, Some(until), outcome).await
    }

    /// Requeues the claim to be claimable at `until`, or marks it failed when `None`.
    async fn settle(
        &self,
        job: Job,
        until: Option<DateTime<Utc>>,
        outcome: Outcome,
    ) -> Result<Outcome, sqlx::Error> {
        let state = if until.is_some() { "queued" } else { "failed" };
        let mut conn = self.pool.acquire().await?;
        Ok(
            if settle_in(&mut conn, job, until, state, self.clock.now()).await? {
                outcome
            } else {
                Outcome::LeaseLost
            },
        )
    }
}

/// Moves a still-held claim to `state`; false if another worker took over its expired lease.
async fn settle_in(
    conn: &mut sqlx::PgConnection,
    job: Job,
    until: Option<DateTime<Utc>>,
    state: &'static str,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE jobs SET state = $3, lease_until = $4, updated_at = $5
         WHERE id = $1 AND attempts = $2 AND state = 'running'",
    )
    .bind(job.id)
    .bind(job.attempts)
    .bind(state)
    .bind(until)
    .bind(now)
    .execute(conn)
    .await?
    .rows_affected()
        == 1)
}

/// Provider cooldown, at least one second so a zero Retry-After cannot spin.
fn cooldown(delay: Duration) -> TimeDelta {
    TimeDelta::from_std(delay)
        .unwrap_or(MAX_RETRY)
        .max(TimeDelta::seconds(1))
}

/// Exponential backoff after the `attempts`-th failed claim (1m, 2m, 4m, … capped at 1h) plus up
/// to 25% jitter, so failed shows do not retry in lockstep.
fn backoff(attempts: i32) -> TimeDelta {
    let doublings = attempts.saturating_sub(1).clamp(0, 16) as u32;
    let base = FIRST_RETRY
        .checked_mul(1 << doublings)
        .unwrap_or(MAX_RETRY)
        .min(MAX_RETRY);
    let mut random = [0u8; 2];
    let _ = getrandom::fill(&mut random);
    let jitter = base.num_milliseconds() / 4 * i64::from(u16::from_le_bytes(random)) / 65_535;
    base + TimeDelta::milliseconds(jitter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_caps_and_jitters_within_a_quarter() {
        for (attempts, base) in [
            (0, 60),
            (1, 60),
            (2, 120),
            (3, 240),
            (7, 3600),
            (i32::MAX, 3600),
        ] {
            let delay = backoff(attempts);
            let base = TimeDelta::seconds(base);
            assert!(
                delay >= base && delay <= base + base / 4,
                "{attempts}: {delay}"
            );
        }
    }

    #[test]
    fn stale_only_after_forty_eight_hours_and_cooldown_never_spins() {
        let fetched = DateTime::<Utc>::from(SystemTime::UNIX_EPOCH);
        assert!(!metadata_stale(fetched, fetched + TimeDelta::hours(48)));
        assert!(metadata_stale(
            fetched,
            fetched + TimeDelta::hours(48) + TimeDelta::seconds(1)
        ));
        assert_eq!(cooldown(Duration::ZERO), TimeDelta::seconds(1));
        assert_eq!(cooldown(Duration::from_secs(120)), TimeDelta::seconds(120));
    }
}
