//! R12 metadata refresh evidence: migrated PostgreSQL, an injected clock and a scripted provider
//! at the external boundary. No provider credentials.
use std::{
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use chrono::{DateTime, NaiveDate, TimeDelta, Utc};
use scenecask_api::{
    domain::{
        progress::{self, ProgressState},
        release::Schedule,
    },
    modules::catalog::{
        import::{ImportRequest, Importer},
        provider::{
            Catalog, Episode, ProviderError, SearchResults, Season, ShowStatus, TvProvider,
        },
        refresh::{Clock, MAX_ATTEMPTS, Pass, Refresher, metadata_stale},
    },
};
use sqlx::PgPool;
use uuid::Uuid;

mod common;
use common::count;

const SHOW: i64 = 77;

/// The provider's current answer for every catalog request.
struct Provider {
    answer: RwLock<Result<Catalog, ProviderError>>,
    calls: AtomicUsize,
}
impl Provider {
    fn new(catalog: Catalog) -> Arc<Self> {
        Arc::new(Self {
            answer: RwLock::new(Ok(catalog)),
            calls: AtomicUsize::new(0),
        })
    }
    fn answer(&self, answer: Result<Catalog, ProviderError>) {
        *self.answer.write().unwrap() = answer;
    }
    fn edit(&self, edit: impl FnOnce(&mut Catalog)) {
        let mut answer = self.answer.write().unwrap();
        let mut catalog = snapshot(&[]);
        if let Ok(current) = answer.as_ref() {
            catalog = current.clone();
        }
        edit(&mut catalog);
        *answer = Ok(catalog);
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}
impl TvProvider for Provider {
    async fn search(&self, _: &str, _: u16) -> Result<SearchResults, ProviderError> {
        Err(ProviderError::Unavailable)
    }
    async fn catalog(&self, _: i64) -> Result<Catalog, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.answer.read().unwrap().clone()
    }
}

struct ManualClock(Mutex<DateTime<Utc>>);
impl ManualClock {
    fn advance(&self, by: TimeDelta) -> DateTime<Utc> {
        let mut now = self.0.lock().unwrap();
        *now += by;
        *now
    }
}
impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

fn day(text: &str) -> Option<NaiveDate> {
    Some(NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap())
}

/// Season 1 episodes as (provider ID, number, air date); every title is protected.
fn snapshot(episodes: &[(i64, i32, Option<NaiveDate>)]) -> Catalog {
    Catalog {
        provider_id: SHOW,
        title: "Lantern Row".into(),
        year: Some(2020),
        genres: vec![],
        synopsis: None,
        poster_path: None,
        status: ShowStatus::Returning,
        seasons: vec![Season {
            provider_id: 701,
            number: 1,
            episodes: episodes
                .iter()
                .map(|&(provider_id, number, air_date)| Episode {
                    provider_id,
                    number,
                    title: Some(format!("Protected title {provider_id}")),
                    overview: Some("Protected overview".into()),
                    still_path: Some("/protected-episode.jpg".into()),
                    air_date,
                })
                .collect(),
        }],
    }
}

struct World {
    pool: PgPool,
    provider: Arc<Provider>,
    clock: Arc<ManualClock>,
    refresher: Arc<Refresher<Provider, Arc<ManualClock>>>,
    show: Uuid,
    user: Uuid,
}
impl World {
    /// Imports the show, saves it for a user who watched `watched` (provider IDs) and starts the
    /// clock at the import time.
    async fn new(
        pool: PgPool,
        episodes: &[(i64, i32, Option<NaiveDate>)],
        watched: &[i64],
    ) -> Self {
        let provider = Provider::new(snapshot(episodes));
        let show = Importer::new(pool.clone(), Arc::clone(&provider))
            .import(&ImportRequest { provider_id: SHOW })
            .await
            .unwrap()
            .show_id;
        let user = common::user(
            &pool,
            &format!("{}@example.test", Uuid::new_v4()),
            true,
            "member",
        )
        .await;
        sqlx::query(
            "INSERT INTO library_entries (user_id, show_id, status, saved_at, revision)
             VALUES ($1, $2, 'watching', now(), 1)",
        )
        .bind(user)
        .bind(show)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO episode_progress (user_id, episode_id, watched, revision)
             SELECT $1, id, true, 3 FROM episodes WHERE tmdb_id = ANY($2)",
        )
        .bind(user)
        .bind(watched)
        .execute(&pool)
        .await
        .unwrap();
        let clock = Arc::new(ManualClock(Mutex::new(fetched_at(&pool).await)));
        let refresher = Arc::new(Refresher::new(
            pool.clone(),
            Arc::clone(&provider),
            Arc::clone(&clock),
        ));
        Self {
            pool,
            provider,
            clock,
            refresher,
            show,
            user,
        }
    }

    async fn pass(&self) -> Pass {
        self.refresher.run_once().await.unwrap()
    }

    async fn scalar(&self, sql: &str) -> i64 {
        count(&self.pool, sql).await
    }

    /// Episode identities and progress, which a refresh must never change.
    async fn history(&self) -> Vec<(Uuid, i64, bool, i64)> {
        sqlx::query_as(
            "SELECT e.id, e.tmdb_id, p.watched, p.revision FROM episode_progress p
             JOIN episodes e ON e.id = p.episode_id ORDER BY e.tmdb_id",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap()
    }

    async fn job(&self) -> (String, i32, Option<DateTime<Utc>>) {
        sqlx::query_as(
            "SELECT state, attempts, lease_until FROM jobs
             WHERE kind = 'metadata_refresh' ORDER BY created_at DESC, id LIMIT 1",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    async fn stale(&self) -> bool {
        metadata_stale(fetched_at(&self.pool).await, self.clock.now())
    }

    /// LibraryItem progress state derived from stored rows at the injected clock.
    async fn progress(&self) -> progress::Progress {
        let rows: Vec<(Uuid, i32, i32, Option<NaiveDate>, bool, bool)> = sqlx::query_as(
            "SELECT e.id, s.number, e.number, e.air_date, e.archived_at IS NOT NULL,
                    coalesce(p.watched, false)
             FROM episodes e JOIN seasons s ON s.id = e.season_id
             LEFT JOIN episode_progress p ON p.episode_id = e.id AND p.user_id = $2
             WHERE e.show_id = $1",
        )
        .bind(self.show)
        .bind(self.user)
        .fetch_all(&self.pool)
        .await
        .unwrap();
        let episodes: Vec<progress::Episode> = rows
            .iter()
            .map(
                |&(id, season, number, air_date, archived, watched)| progress::Episode {
                    id,
                    season,
                    number,
                    schedule: air_date.map_or(Schedule::Undated, Schedule::Date),
                    archived,
                    watched,
                },
            )
            .collect();
        let (status, complete_import): (String, bool) =
            sqlx::query_as("SELECT status, complete_import FROM shows")
                .fetch_one(&self.pool)
                .await
                .unwrap();
        let status = match status.as_str() {
            "returning" => progress::ShowStatus::Returning,
            "ended" => progress::ShowStatus::Ended,
            "canceled" => progress::ShowStatus::Canceled,
            _ => progress::ShowStatus::Unknown,
        };
        let show = progress::Show {
            status,
            complete_import,
            episodes: &episodes,
        };
        progress::progress(&show, self.clock.now())
    }
}

const PAST: &str = "2020-01-01";
const FUTURE: &str = "2999-01-01";

#[sqlx::test]
#[tracing_test::traced_test]
async fn outage_and_incomplete_snapshots_keep_last_good_catalog_history_and_disclose_staleness(
    pool: PgPool,
) {
    let world = World::new(pool, &[(1, 1, day(PAST)), (2, 2, day(FUTURE))], &[1]).await;
    let history = world.history().await;
    let catalog = world.scalar("SELECT catalog_revision FROM shows").await;
    let before = world.progress().await;
    assert_eq!(before.state, ProgressState::CaughtUp);

    // Not due within 24 hours.
    world.clock.advance(TimeDelta::hours(23));
    assert_eq!(world.pass().await, Pass::default());
    assert_eq!(world.provider.calls(), 1);

    world.provider.answer(Err(ProviderError::Unavailable));
    world.clock.advance(TimeDelta::hours(2));
    let pass = world.pass().await;
    assert_eq!((pass.scheduled, pass.retried), (1, 1));
    let (state, attempts, retry_at) = world.job().await;
    assert_eq!((state.as_str(), attempts), ("queued", 1));
    assert!(retry_at.unwrap() >= world.clock.now() + TimeDelta::minutes(1));
    // Backoff: nothing is claimed before the retry time.
    assert_eq!(world.pass().await, Pass::default());
    assert_eq!(world.provider.calls(), 2);

    // An incomplete/invalid snapshot (missing season, duplicate identity) is also rejected
    // before any write.
    world.provider.answer(Err(ProviderError::InvalidData));
    for attempt in 2..=MAX_ATTEMPTS {
        world
            .clock
            .advance(TimeDelta::hours(1) + TimeDelta::minutes(16));
        let pass = world.pass().await;
        let expected = if attempt == MAX_ATTEMPTS {
            (0, 1)
        } else {
            (1, 0)
        };
        assert_eq!((pass.retried, pass.failed), expected, "attempt {attempt}");
    }
    assert_eq!(world.job().await.0, "failed");
    assert!(!world.stale().await, "stale only after 48 hours");
    assert_eq!(world.history().await, history);
    assert_eq!(
        world.scalar("SELECT catalog_revision FROM shows").await,
        catalog
    );
    assert_eq!(
        world
            .scalar("SELECT count(*) FROM shows WHERE complete_import")
            .await,
        1
    );
    assert_eq!(
        world
            .scalar("SELECT count(*) FROM episodes WHERE archived_at IS NULL")
            .await,
        2
    );
    assert_eq!(world.progress().await, before);

    // An exhausted show waits a full interval before it is scheduled again.
    // 48h20m after the last good fetch; the failed job was created at 25h.
    world.clock.advance(TimeDelta::hours(17));
    assert_eq!(world.pass().await, Pass::default());
    assert!(
        world.stale().await,
        "48h without a successful refresh is disclosed"
    );
    assert_eq!(
        world.progress().await,
        before,
        "saved catalog and history stay usable"
    );

    world.clock.advance(TimeDelta::hours(1));
    world
        .provider
        .answer(Ok(snapshot(&[(1, 1, day(PAST)), (2, 2, day(FUTURE))])));
    let pass = world.pass().await;
    assert_eq!((pass.scheduled, pass.refreshed), (1, 1));
    assert!(!world.stale().await);
    assert_eq!(world.history().await, history);
    assert_eq!(
        world.scalar("SELECT catalog_revision FROM shows").await,
        catalog,
        "an unchanged snapshot keeps previews valid"
    );
    assert!(!logs_contain("Protected"));
    assert!(!logs_contain("protected-episode"));
}

#[sqlx::test]
async fn newly_released_and_discovered_episodes_move_caught_up_show_to_in_progress(pool: PgPool) {
    let world = World::new(pool, &[(1, 1, day(PAST)), (2, 2, day(FUTURE))], &[1]).await;
    assert_eq!(world.progress().await.state, ProgressState::CaughtUp);
    let catalog = world.scalar("SELECT catalog_revision FROM shows").await;

    // The provider moves E2's date into the past and lists a new E3; the show has ended.
    world.provider.edit(|catalog| {
        catalog.status = ShowStatus::Ended;
        catalog.seasons[0].episodes[1].air_date = day("2021-01-01");
        catalog.seasons[0]
            .episodes
            .push(snapshot(&[(3, 3, day("2021-02-01"))]).seasons[0].episodes[0].clone());
    });
    world.clock.advance(TimeDelta::hours(25));
    assert_eq!(world.pass().await.refreshed, 1);
    let progress = world.progress().await;
    assert_eq!(progress.state, ProgressState::InProgress);
    assert_eq!((progress.watched, progress.total), (1, 3));
    assert_eq!(
        progress.next_episode.map(|e| (e.season, e.number)),
        Some((1, 2))
    );
    assert!(world.scalar("SELECT catalog_revision FROM shows").await > catalog);
    assert_eq!(
        world
            .scalar("SELECT count(*) FROM shows WHERE status = 'ended'")
            .await,
        1
    );
}

#[sqlx::test]
async fn renumbering_and_removal_keep_history_identity_and_stale_catchup_previews(pool: PgPool) {
    let world = World::new(
        pool,
        &[(1, 1, day(PAST)), (2, 2, day(PAST)), (3, 3, day(PAST))],
        &[1, 3],
    )
    .await;
    let history = world.history().await;
    sqlx::query(
        "INSERT INTO catchup_previews
             (user_id, show_id, endpoint_episode_id, episode_ids, revisions, catalog_revision, expires_at)
         SELECT $1, $2, id, jsonb_build_array(id), '{}', (SELECT catalog_revision FROM shows),
                now() + interval '5 minutes'
         FROM episodes WHERE tmdb_id = 2",
    )
    .bind(world.user)
    .bind(world.show)
    .execute(&world.pool)
    .await
    .unwrap();

    // Swap E1/E2 numbers and remove the watched E3.
    world
        .provider
        .answer(Ok(snapshot(&[(1, 2, day(PAST)), (2, 1, day(PAST))])));
    world.clock.advance(TimeDelta::hours(25));
    assert_eq!(world.pass().await.refreshed, 1);

    assert_eq!(
        world.history().await,
        history,
        "progress keeps episode identity and revisions"
    );
    let numbers: Vec<(i64, i32, bool)> = sqlx::query_as(
        "SELECT tmdb_id, number, archived_at IS NOT NULL FROM episodes ORDER BY tmdb_id",
    )
    .fetch_all(&world.pool)
    .await
    .unwrap();
    assert_eq!(numbers, [(1, 2, false), (2, 1, false), (3, 3, true)]);
    assert_eq!(
        world
            .scalar(
                "SELECT count(*) FROM catchup_previews p JOIN shows s ON s.id = p.show_id
             WHERE p.catalog_revision < s.catalog_revision"
            )
            .await,
        1,
        "the preview's catalog revision is now stale"
    );
    // The archived watched episode leaves active progress; E2 (now number 1) is next.
    let progress = world.progress().await;
    assert_eq!((progress.watched, progress.total), (1, 2));
    assert_eq!(progress.next_episode.map(|e| e.number), Some(1));
    assert!(progress.out_of_order);
}

#[sqlx::test]
async fn identity_collision_rolls_back_the_whole_refresh_and_retries(pool: PgPool) {
    let world = World::new(pool, &[(1, 1, day(PAST)), (2, 2, day(FUTURE))], &[1]).await;
    // Another show already owns provider episode 900.
    let other: Uuid = sqlx::query_scalar(
        "INSERT INTO shows (tmdb_id, title, fetched_at, complete_import) VALUES (78, 'Other', now(), true)
         RETURNING id",
    )
    .fetch_one(&world.pool)
    .await
    .unwrap();
    sqlx::query(
        "WITH s AS (INSERT INTO seasons (tmdb_id, show_id, number) VALUES (780, $1, 1) RETURNING id)
         INSERT INTO episodes (tmdb_id, show_id, season_id, number) SELECT 900, $1, id, 1 FROM s",
    )
    .bind(other)
    .execute(&world.pool)
    .await
    .unwrap();
    let catalog = world
        .scalar("SELECT catalog_revision FROM shows WHERE tmdb_id = 77")
        .await;
    let history = world.history().await;
    let fetched = fetched_at(&world.pool).await;

    // A renumbering that would apply in place, plus the colliding identity.
    world.provider.answer(Ok(snapshot(&[
        (1, 2, day(PAST)),
        (2, 1, day(PAST)),
        (900, 3, day(PAST)),
    ])));
    world.clock.advance(TimeDelta::hours(25));
    let pass = world.pass().await;
    assert_eq!((pass.refreshed, pass.retried), (0, 1));
    assert_eq!(world.job().await.0, "queued");
    assert_eq!(
        world
            .scalar("SELECT catalog_revision FROM shows WHERE tmdb_id = 77")
            .await,
        catalog
    );
    let numbers: Vec<(i64, i32)> =
        sqlx::query_as("SELECT tmdb_id, number FROM episodes WHERE show_id = $1 ORDER BY tmdb_id")
            .bind(world.show)
            .fetch_all(&world.pool)
            .await
            .unwrap();
    assert_eq!(numbers, [(1, 1), (2, 2)], "in-place updates rolled back");
    assert_eq!(world.history().await, history);
    assert_eq!(fetched_at(&world.pool).await, fetched);
}

async fn fetched_at(pool: &PgPool) -> DateTime<Utc> {
    sqlx::query_scalar("SELECT fetched_at FROM shows WHERE tmdb_id = 77")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn crashed_leases_recover_and_provider_cooldowns_pause_the_worker(pool: PgPool) {
    let world = World::new(pool, &[(1, 1, day(PAST))], &[1]).await;
    world.clock.advance(TimeDelta::hours(25));
    // A worker claimed the job and crashed; its lease is still running.
    sqlx::query(
        "INSERT INTO jobs (kind, state, payload, attempts, lease_until)
         VALUES ('metadata_refresh', 'running', jsonb_build_object('showId', $1::uuid), 1, $2)",
    )
    .bind(world.show)
    .bind(world.clock.now() + TimeDelta::minutes(1))
    .execute(&world.pool)
    .await
    .unwrap();
    assert_eq!(
        world.pass().await,
        Pass::default(),
        "a live lease is neither claimed nor duplicated"
    );
    assert_eq!(world.provider.calls(), 1);

    // After the lease ends the job is reclaimed; the provider asks for a two-minute cooldown.
    world
        .provider
        .answer(Err(ProviderError::RateLimited(Duration::from_secs(120))));
    let now = world.clock.advance(TimeDelta::minutes(2));
    let pass = world.pass().await;
    assert_eq!((pass.scheduled, pass.rate_limited), (0, 1));
    let (state, attempts, retry_at) = world.job().await;
    assert_eq!((state.as_str(), attempts), ("queued", 2));
    assert_eq!(retry_at, Some(now + TimeDelta::minutes(2)));

    // During the cooldown nothing reaches the provider, even another due show.
    world.provider.answer(Ok(snapshot(&[(1, 1, day(PAST))])));
    world.clock.advance(TimeDelta::minutes(1));
    let pass = world.pass().await;
    assert!(pass.paused);
    assert_eq!(world.provider.calls(), 2);

    world.clock.advance(TimeDelta::minutes(1));
    assert_eq!(world.pass().await.refreshed, 1);
    assert_eq!(world.job().await, ("ready".into(), 3, None));
    assert_eq!(world.scalar("SELECT count(*) FROM jobs").await, 1);

    // An exhausted claim that crashed is settled as failed instead of being retried again.
    sqlx::query("UPDATE jobs SET state = 'running', attempts = $1, lease_until = $2")
        .bind(MAX_ATTEMPTS)
        .bind(world.clock.now())
        .execute(&world.pool)
        .await
        .unwrap();
    world.clock.advance(TimeDelta::seconds(1));
    world.pass().await;
    assert_eq!(world.job().await.0, "failed");
    assert_eq!(world.provider.calls(), 3);
}

#[sqlx::test]
async fn only_saved_shows_are_scheduled_once_and_old_jobs_are_pruned(pool: PgPool) {
    let world = World::new(pool, &[(1, 1, day(PAST))], &[]).await;
    sqlx::query("UPDATE library_entries SET saved = false, saved_at = NULL, revision = 2")
        .execute(&world.pool)
        .await
        .unwrap();
    world.clock.advance(TimeDelta::hours(25));
    assert_eq!(world.pass().await.scheduled, 0);

    sqlx::query("UPDATE library_entries SET saved = true, saved_at = now(), revision = 3")
        .execute(&world.pool)
        .await
        .unwrap();
    world.provider.answer(Err(ProviderError::Unavailable));
    let refresher = Arc::clone(&world.refresher);
    let (a, b) = tokio::join!(refresher.run_once(), world.refresher.run_once());
    assert_eq!(
        a.unwrap().scheduled + b.unwrap().scheduled,
        1,
        "concurrent schedulers converge"
    );
    assert_eq!(world.scalar("SELECT count(*) FROM jobs").await, 1);

    sqlx::query("UPDATE jobs SET state = 'failed', lease_until = NULL")
        .execute(&world.pool)
        .await
        .unwrap();
    world.clock.advance(TimeDelta::days(8));
    world.pass().await;
    assert_eq!(
        world
            .scalar("SELECT count(*) FROM jobs WHERE state = 'failed'")
            .await,
        0,
        "finished jobs older than the retention are pruned"
    );
    assert_eq!(world.scalar("SELECT count(*) FROM jobs").await, 1);
}
