//! R11 integration evidence uses migrated PostgreSQL, never SQLite or provider credentials.
use scenecask_api::{
    error::ErrorCode,
    modules::catalog::{
        import::{ImportRequest, Importer},
        provider::{
            Catalog, Episode, ProviderError, Search, SearchQuery, SearchResults, SearchShow,
            Season, ShowStatus, TvProvider,
        },
    },
};
use sqlx::PgPool;
use std::{
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;

struct Fixture {
    catalog: RwLock<Catalog>,
    failed: AtomicBool,
    imports: AtomicUsize,
    searches: AtomicUsize,
}
impl Fixture {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            catalog: RwLock::new(catalog()),
            failed: AtomicBool::new(false),
            imports: AtomicUsize::new(0),
            searches: AtomicUsize::new(0),
        })
    }
}
fn catalog() -> Catalog {
    Catalog {
        provider_id: 123,
        title: "Hollow Orchard".into(),
        year: Some(2024),
        genres: vec!["Drama".into()],
        synopsis: Some("General series synopsis".into()),
        poster_path: None,
        status: ShowStatus::Returning,
        seasons: vec![
            Season {
                provider_id: 1001,
                number: 1,
                episodes: vec![
                    Episode {
                        provider_id: 2001,
                        number: 1,
                        title: Some("Protected episode".into()),
                        overview: Some("Protected overview".into()),
                        still_path: Some("/protected-episode.jpg".into()),
                        air_date: None,
                    },
                    Episode {
                        provider_id: 2002,
                        number: 2,
                        title: None,
                        overview: None,
                        still_path: None,
                        air_date: Some(chrono::NaiveDate::from_ymd_opt(2999, 1, 1).unwrap()),
                    },
                ],
            },
            Season {
                provider_id: 1000,
                number: 0,
                episodes: vec![Episode {
                    provider_id: 2000,
                    number: 1,
                    title: Some("Protected special".into()),
                    overview: None,
                    still_path: None,
                    air_date: None,
                }],
            },
        ],
    }
}
impl TvProvider for Fixture {
    async fn search(&self, _: &str, page: u16) -> Result<SearchResults, ProviderError> {
        self.searches.fetch_add(1, Ordering::SeqCst);
        if self.failed.load(Ordering::SeqCst) {
            return Err(ProviderError::Unavailable);
        }
        Ok(SearchResults {
            items: vec![SearchShow {
                provider: "tmdb",
                provider_id: 123,
                title: "Hollow Orchard".into(),
                year: Some(2024),
                genres: vec!["Drama".into()],
                poster_url: None,
            }],
            page,
            total_pages: 2,
        })
    }
    async fn catalog(&self, _: i64) -> Result<Catalog, ProviderError> {
        self.imports.fetch_add(1, Ordering::SeqCst);
        // Make concurrent requests overlap at the external boundary.
        tokio::time::sleep(Duration::from_millis(10)).await;
        if self.failed.load(Ordering::SeqCst) {
            return Err(ProviderError::Unavailable);
        }
        Ok(self.catalog.read().unwrap().clone())
    }
}
async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(pool)
        .await
        .unwrap()
}
async fn stale(pool: &PgPool) {
    sqlx::query("UPDATE shows SET fetched_at = now() - interval '25 hours'")
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test]
async fn concurrent_imports_converge_and_recent_imports_survive_provider_failure(pool: PgPool) {
    let provider = Fixture::new();
    let importer = Arc::new(Importer::new(pool.clone(), Arc::clone(&provider)));
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let importer = Arc::clone(&importer);
        tasks.spawn(async move {
            importer
                .import(&ImportRequest { provider_id: 123 })
                .await
                .unwrap()
        });
    }
    let mut ids = Vec::new();
    while let Some(result) = tasks.join_next().await {
        ids.push(result.unwrap().show_id);
    }
    assert!(ids.iter().all(|id| *id == ids[0]));
    assert_eq!(count(&pool, "SELECT count(*) FROM shows").await, 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM seasons").await, 2);
    assert_eq!(count(&pool, "SELECT count(*) FROM episodes").await, 3);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM shows WHERE complete_import AND poster_path IS NULL"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM episodes WHERE air_date IS NULL AND release_timezone IS NULL"
        )
        .await,
        2
    );
    // An unchanged refresh must not invalidate catch-up previews.
    let revision_sql = "SELECT catalog_revision FROM shows WHERE tmdb_id = 123";
    let revision = count(&pool, revision_sql).await;
    stale(&pool).await;
    importer
        .import(&ImportRequest { provider_id: 123 })
        .await
        .unwrap();
    assert_eq!(count(&pool, revision_sql).await, revision);
    let imported_calls = provider.imports.load(Ordering::SeqCst);
    provider.failed.store(true, Ordering::SeqCst);
    assert_eq!(
        importer
            .import(&ImportRequest { provider_id: 123 })
            .await
            .unwrap()
            .show_id,
        ids[0]
    );
    assert_eq!(provider.imports.load(Ordering::SeqCst), imported_calls);
    let serialized = serde_json::to_string(
        &importer
            .import(&ImportRequest { provider_id: 123 })
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&serialized).unwrap(),
        serde_json::json!({"showId": ids[0]})
    );
    assert_ne!(ids[0], Uuid::nil());
}

#[sqlx::test]
async fn provider_failure_and_incomplete_snapshot_never_make_initial_catalog_usable(pool: PgPool) {
    let provider = Fixture::new();
    let importer = Importer::new(pool.clone(), Arc::clone(&provider));
    provider.failed.store(true, Ordering::SeqCst);
    assert_eq!(
        importer
            .import(&ImportRequest { provider_id: 123 })
            .await
            .unwrap_err()
            .code(),
        ErrorCode::ProviderUnavailable
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM shows").await, 0);
    provider.failed.store(false, Ordering::SeqCst);
    provider.catalog.write().unwrap().seasons[0].episodes[1].number = 1;
    assert_eq!(
        importer
            .import(&ImportRequest { provider_id: 123 })
            .await
            .unwrap_err()
            .code(),
        ErrorCode::ProviderUnavailable
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM shows").await, 0);
    assert_eq!(
        importer
            .import(&ImportRequest { provider_id: 0 })
            .await
            .unwrap_err()
            .code(),
        ErrorCode::ValidationError
    );
}

#[sqlx::test]
async fn cross_show_identity_collision_rolls_back_every_initial_write(pool: PgPool) {
    let provider = Fixture::new();
    let importer = Importer::new(pool.clone(), Arc::clone(&provider));
    let original = importer
        .import(&ImportRequest { provider_id: 123 })
        .await
        .unwrap()
        .show_id;
    {
        let mut data = provider.catalog.write().unwrap();
        data.provider_id = 124;
        data.seasons[0].provider_id = 1002; // first episode collides after a new show + season have been inserted
        data.seasons.truncate(1);
    }
    assert_eq!(
        importer
            .import(&ImportRequest { provider_id: 124 })
            .await
            .unwrap_err()
            .code(),
        ErrorCode::ProviderUnavailable
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM shows").await, 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM seasons").await, 2);
    assert_eq!(count(&pool, "SELECT count(*) FROM episodes").await, 3);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM seasons WHERE tmdb_id = 1002").await,
        0
    );
    let id: Uuid = sqlx::query_scalar("SELECT id FROM shows WHERE tmdb_id = 123")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(id, original);
}

#[sqlx::test]
async fn corrections_archival_restore_and_failed_reimport_preserve_watched_history(pool: PgPool) {
    let provider = Fixture::new();
    let importer = Importer::new(pool.clone(), Arc::clone(&provider));
    let id = importer
        .import(&ImportRequest { provider_id: 123 })
        .await
        .unwrap()
        .show_id;
    let episodes: Vec<(i64, Uuid)> =
        sqlx::query_as("SELECT tmdb_id, id FROM episodes ORDER BY tmdb_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    let user: Uuid = sqlx::query_scalar(
        "INSERT INTO users (normalized_email) VALUES ('fixture@example.test') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO episode_progress (user_id, episode_id, watched, revision) VALUES ($1, $2, true, 7)").bind(user).bind(episodes[1].1).execute(&pool).await.unwrap();
    let before_revision = count(
        &pool,
        "SELECT catalog_revision FROM shows WHERE tmdb_id = 123",
    )
    .await;
    stale(&pool).await;
    // Swap the two regular episode numbers using stable provider IDs.
    {
        let mut data = provider.catalog.write().unwrap();
        data.seasons[0].episodes[0].number = 2;
        data.seasons[0].episodes[1].number = 1;
    }
    assert_eq!(
        importer
            .import(&ImportRequest { provider_id: 123 })
            .await
            .unwrap()
            .show_id,
        id
    );
    assert_eq!(
        sqlx::query_as::<_, (i64, Uuid)>("SELECT tmdb_id, id FROM episodes ORDER BY tmdb_id")
            .fetch_all(&pool)
            .await
            .unwrap(),
        episodes
    );
    assert!(
        count(
            &pool,
            "SELECT catalog_revision FROM shows WHERE tmdb_id = 123"
        )
        .await
            > before_revision
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM episode_progress WHERE watched AND revision = 7"
        )
        .await,
        1
    );
    stale(&pool).await;
    let removed = provider.catalog.write().unwrap().seasons[0]
        .episodes
        .remove(0);
    importer
        .import(&ImportRequest { provider_id: 123 })
        .await
        .unwrap();
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM episodes WHERE tmdb_id = 2001 AND archived_at IS NOT NULL"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM episode_progress WHERE watched AND revision = 7"
        )
        .await,
        1
    );
    stale(&pool).await;
    provider.catalog.write().unwrap().seasons[0]
        .episodes
        .push(removed);
    importer
        .import(&ImportRequest { provider_id: 123 })
        .await
        .unwrap();
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM episodes WHERE tmdb_id = 2001 AND archived_at IS NULL"
        )
        .await,
        1
    );
    stale(&pool).await;
    let revision = count(
        &pool,
        "SELECT catalog_revision FROM shows WHERE tmdb_id = 123",
    )
    .await;
    provider.failed.store(true, Ordering::SeqCst);
    assert_eq!(
        importer
            .import(&ImportRequest { provider_id: 123 })
            .await
            .unwrap()
            .show_id,
        id
    );
    assert_eq!(
        count(
            &pool,
            "SELECT catalog_revision FROM shows WHERE tmdb_id = 123"
        )
        .await,
        revision
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM episode_progress WHERE watched AND revision = 7"
        )
        .await,
        1
    );
}

#[tokio::test(start_paused = true)]
async fn cache_validates_queries_expires_and_does_not_cache_failure() {
    let provider = Fixture::new();
    let search = Search::new(Arc::clone(&provider));
    let query = SearchQuery {
        q: "  Hollow Orchard  ".into(),
        page: 1,
    };
    search.search(&query).await.unwrap();
    search
        .search(&SearchQuery {
            q: "Hollow Orchard".into(),
            page: 1,
        })
        .await
        .unwrap();
    assert_eq!(provider.searches.load(Ordering::SeqCst), 1);
    tokio::time::advance(Duration::from_secs(900)).await;
    provider.failed.store(true, Ordering::SeqCst);
    assert_eq!(
        search.search(&query).await.unwrap_err().code(),
        ErrorCode::ProviderUnavailable
    );
    provider.failed.store(false, Ordering::SeqCst);
    search.search(&query).await.unwrap();
    assert_eq!(provider.searches.load(Ordering::SeqCst), 3);
    search
        .search(&SearchQuery {
            q: "Hollow Orchard".into(),
            page: 2,
        })
        .await
        .unwrap();
    assert_eq!(provider.searches.load(Ordering::SeqCst), 4);
    for (q, page) in [
        ("x".into(), 1),
        (" ".into(), 1),
        ("x".repeat(101), 1),
        ("ok".into(), 0),
        ("ok".into(), 501),
    ] {
        assert_eq!(
            search
                .search(&SearchQuery { q, page })
                .await
                .unwrap_err()
                .code(),
            ErrorCode::ValidationError
        );
    }
    assert_eq!(provider.searches.load(Ordering::SeqCst), 4);
    let request =
        serde_json::from_str::<ImportRequest>(r#"{"providerId":123,"unexpected":"secret"}"#);
    assert!(request.is_err());
}

fn http_app(
    pool: &PgPool,
    provider: Arc<Fixture>,
) -> (axum::Router, scenecask_api::middleware::Security) {
    use axum::{
        Router,
        routing::{get, post},
    };
    use scenecask_api::{
        middleware::{
            self, Security, SecurityConfig,
            rate_limit::{RateLimiter, Rule},
        },
        modules::catalog::{
            import::import_handler,
            provider::{SearchState, search_handler},
        },
    };
    let security = Security::new(
        pool.clone(),
        SecurityConfig::new("https://scenecask.example").unwrap(),
    );
    let search = Router::new()
        .route("/shows/search", get(search_handler::<Fixture>))
        .with_state(SearchState {
            search: Arc::new(Search::new(Arc::clone(&provider))),
            limiter: RateLimiter::new(),
            rule: Rule::per_minute(30),
        });
    let import = Router::new()
        .route("/shows/import", post(import_handler::<Fixture>))
        .with_state(Arc::new(Importer::new(pool.clone(), provider)));
    (middleware::apply(search.merge(import), &security), security)
}
async fn signed(
    security: &scenecask_api::middleware::Security,
    verified: bool,
) -> (String, String) {
    let id: Uuid = sqlx::query_scalar("INSERT INTO users (normalized_email, verified_at) VALUES ($1, CASE WHEN $2 THEN now() END) RETURNING id")
        .bind(format!("{}@example.test", Uuid::new_v4())).bind(true).fetch_one(&security.pool).await.unwrap();
    let mut conn = security.pool.acquire().await.unwrap();
    let started =
        scenecask_api::modules::auth::session::start_session(&mut conn, &security.config, None, id)
            .await
            .unwrap();
    if !verified {
        sqlx::query("UPDATE users SET verified_at = NULL WHERE id = $1")
            .bind(id)
            .execute(&security.pool)
            .await
            .unwrap();
    }
    (
        started
            .cookie
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned(),
        started.session.csrf_token,
    )
}
async fn http_call(
    app: &axum::Router,
    path: &str,
    session: Option<&(String, String)>,
    body: Option<&str>,
    csrf: bool,
) -> (axum::http::StatusCode, serde_json::Value) {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let mut request = Request::builder().uri(path);
    if let Some((cookie, token)) = session {
        request = request.header("cookie", cookie);
        if csrf {
            request = request.header("x-csrf-token", token);
        }
    }
    if body.is_some() {
        request = request
            .method("POST")
            .header("origin", "https://scenecask.example")
            .header("content-type", "application/json");
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(Body::from(body.unwrap_or("").to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[sqlx::test]
async fn real_http_handlers_enforce_auth_verification_csrf_validation_and_redaction(pool: PgPool) {
    use axum::http::StatusCode;
    let provider = Fixture::new();
    let (app, security) = http_app(&pool, Arc::clone(&provider));
    assert_eq!(
        http_call(&app, "/shows/search?q=orchard", None, None, false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        http_call(
            &app,
            "/shows/import",
            None,
            Some(r#"{"providerId":123}"#),
            false
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let unverified = signed(&security, false).await;
    assert_eq!(
        http_call(
            &app,
            "/shows/search?q=orchard",
            Some(&unverified),
            None,
            false
        )
        .await
        .1["error"]["code"],
        "EMAIL_UNVERIFIED"
    );
    assert_eq!(
        http_call(
            &app,
            "/shows/import",
            Some(&unverified),
            Some(r#"{"providerId":123}"#),
            true
        )
        .await
        .1["error"]["code"],
        "EMAIL_UNVERIFIED"
    );
    let session = signed(&security, true).await;
    assert_eq!(
        http_call(
            &app,
            "/shows/import",
            Some(&session),
            Some(r#"{"providerId":123}"#),
            false
        )
        .await
        .1["error"]["code"],
        "CSRF_FAILED"
    );
    assert_eq!(provider.imports.load(Ordering::SeqCst), 0);
    for (path, field) in [
        ("/shows/search", "q"),
        ("/shows/search?q=x", "q"),
        ("/shows/search?q=orchard&page=bad", "page"),
        ("/shows/search?q=orchard&page=70000", "page"),
        ("/shows/search?q=orchard&page=501", "page"),
    ] {
        let (status, error) = http_call(&app, path, Some(&session), None, false).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{path}");
        assert!(error["error"]["fields"].get(field).is_some(), "{path}");
    }
    let (status, results) =
        http_call(&app, "/shows/search?q=orchard", Some(&session), None, false).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(results["items"][0]["providerId"], 123);
    assert_eq!(results["items"][0]["posterUrl"], serde_json::Value::Null);
    for body in [
        r#"{"providerId":123,"unknown":"Protected overview"}"#,
        r#"{"providerId":0}"#,
    ] {
        let (status, json) =
            http_call(&app, "/shows/import", Some(&session), Some(body), true).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(!json.to_string().contains("Protected"));
    }
    let (status, result) = http_call(
        &app,
        "/shows/import",
        Some(&session),
        Some(r#"{"providerId":123}"#),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result.as_object().unwrap().len(), 1);
    assert!(Uuid::parse_str(result["showId"].as_str().unwrap()).is_ok());
    assert!(!result.to_string().contains("Protected"));
    stale(&pool).await;
    provider.failed.store(true, Ordering::SeqCst);
    let (status, reused) = http_call(
        &app,
        "/shows/import",
        Some(&session),
        Some(r#"{"providerId":123}"#),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reused["showId"], result["showId"]);
    let (status, error) = http_call(
        &app,
        "/shows/import",
        Some(&session),
        Some(r#"{"providerId":777}"#),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(error["error"]["code"], "PROVIDER_UNAVAILABLE");
    assert_eq!(
        count(&pool, "SELECT count(*) FROM shows WHERE complete_import").await,
        1
    );
}

#[sqlx::test]
async fn searches_are_rate_limited_even_when_results_are_cached(pool: PgPool) {
    use axum::http::StatusCode;
    let provider = Fixture::new();
    let (app, security) = http_app(&pool, Arc::clone(&provider));
    let session = signed(&security, true).await;
    for _ in 0..30 {
        assert_eq!(
            http_call(&app, "/shows/search?q=orchard", Some(&session), None, false)
                .await
                .0,
            StatusCode::OK
        );
    }
    let (status, error) =
        http_call(&app, "/shows/search?q=orchard", Some(&session), None, false).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error["error"]["code"], "RATE_LIMITED");
    assert_eq!(provider.searches.load(Ordering::SeqCst), 1);
    let other = signed(&security, true).await;
    assert_eq!(
        http_call(&app, "/shows/search?q=orchard", Some(&other), None, false)
            .await
            .0,
        StatusCode::OK
    );
}

#[sqlx::test]
async fn failed_stale_import_rolls_back_archival_and_in_place_updates(pool: PgPool) {
    let provider = Fixture::new();
    let importer = Importer::new(pool.clone(), Arc::clone(&provider));
    let id = importer
        .import(&ImportRequest { provider_id: 123 })
        .await
        .unwrap()
        .show_id;
    sqlx::raw_sql("INSERT INTO shows (tmdb_id, title, fetched_at, complete_import) VALUES (999, 'Other show', now(), true);
        INSERT INTO seasons (tmdb_id, show_id, number) SELECT 9990, id, 1 FROM shows WHERE tmdb_id = 999;
        INSERT INTO episodes (tmdb_id, show_id, season_id, number) SELECT 9991, show_id, id, 1 FROM seasons WHERE tmdb_id = 9990;").execute(&pool).await.unwrap();
    stale(&pool).await;
    let revision = count(
        &pool,
        "SELECT catalog_revision FROM shows WHERE tmdb_id = 123",
    )
    .await;
    {
        let mut snapshot = provider.catalog.write().unwrap();
        snapshot.seasons.truncate(1); // the special would be archived
        snapshot.seasons[0].episodes[0].title = Some("Replacement protected title".into());
        snapshot.seasons[0].episodes[1].provider_id = 9991; // collides after the first episode update
    }
    // The rejected refresh rolls back and the last good catalog stays usable.
    assert_eq!(
        importer
            .import(&ImportRequest { provider_id: 123 })
            .await
            .unwrap()
            .show_id,
        id
    );
    assert_eq!(
        count(
            &pool,
            "SELECT catalog_revision FROM shows WHERE tmdb_id = 123"
        )
        .await,
        revision
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM episodes WHERE archived_at IS NOT NULL"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM episodes WHERE tmdb_id = 2001 AND title = 'Protected episode'"
        )
        .await,
        1
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM shows WHERE complete_import").await,
        2
    );
}
