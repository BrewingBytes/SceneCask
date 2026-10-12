//! R24 application integration: composes every module router under `/api/v1` behind one set of
//! C03 security layers, and starts the background workers. Feature behavior stays in modules.

use std::{future::Future, sync::Arc, time::Duration};

use axum::{
    Router,
    routing::{get, post},
};
use sqlx::PgPool;
use tokio::sync::watch;

use crate::{
    middleware::{self, Security, SecurityConfig, rate_limit::SEARCH_RULES},
    modules::{
        account,
        auth::{self, AuthMail, google::Google},
        catalog::{
            import::{Importer, import_handler},
            provider::{
                Catalog, ProviderError, Search, SearchResults, SearchState, TvProvider,
                search_handler, tmdb::Tmdb,
            },
            refresh::{Refresher, SystemClock},
            views,
        },
        library,
        mail::{Worker, config::MailConfig},
        policy, tracking,
    },
    router,
};

/// The metadata provider: TMDB when a token is configured; otherwise every provider call fails
/// as unavailable, so search/import answer 502 and saved catalog data keeps serving.
pub enum MetadataProvider {
    Tmdb(Tmdb),
    Unconfigured,
}

impl TvProvider for MetadataProvider {
    async fn search(&self, query: &str, page: u16) -> Result<SearchResults, ProviderError> {
        match self {
            Self::Tmdb(tmdb) => tmdb.search(query, page).await,
            Self::Unconfigured => Err(ProviderError::Unavailable),
        }
    }

    async fn catalog(&self, provider_id: i64) -> Result<Catalog, ProviderError> {
        match self {
            Self::Tmdb(tmdb) => tmdb.catalog(provider_id).await,
            Self::Unconfigured => Err(ProviderError::Unavailable),
        }
    }
}

/// Long-lived shared state. Build once per process so caches and rate limits are shared.
#[derive(Clone)]
pub struct Services {
    pub security: Security,
    pub provider: Arc<MetadataProvider>,
    pub google: Option<Arc<Google>>,
    /// Social beta routes are mounted only when enabled; otherwise they are 404 like any
    /// unknown path (C06).
    pub beta: bool,
}

impl Services {
    pub fn new(
        pool: PgPool,
        security: SecurityConfig,
        provider: MetadataProvider,
        google: Option<Google>,
        beta: bool,
    ) -> Self {
        Self {
            security: Security::new(pool, security),
            provider: Arc::new(provider),
            google: google.map(Arc::new),
            beta,
        }
    }
}

/// Health checks at the root plus the full `/api/v1` API.
pub fn application(services: &Services) -> Router {
    router(services.security.pool.clone()).nest("/api/v1", api(services))
}

/// Every alpha route, relative to `/api/v1`, with the C03 layers applied once.
pub fn api(services: &Services) -> Router {
    let security = &services.security;
    let search = SearchState {
        search: Arc::new(Search::new(Arc::clone(&services.provider))),
        limiter: security.limiter.clone(),
        rule: SEARCH_RULES[0],
    };
    let importer = Arc::new(Importer::new(
        security.pool.clone(),
        Arc::clone(&services.provider),
    ));
    let mut routes = Router::new()
        .merge(auth::session::routes(security.clone()))
        .merge(auth::registration::routes(security.clone()))
        .merge(auth::password::routes(security.clone()))
        .merge(account::profile::routes(security.clone()))
        .merge(
            Router::new()
                .route("/shows/search", get(search_handler::<MetadataProvider>))
                .with_state(search),
        )
        .merge(
            Router::new()
                .route("/shows/import", post(import_handler::<MetadataProvider>))
                .with_state(importer),
        )
        .merge(views::routes(security.clone()))
        .merge(library::routes(security.clone()))
        .merge(tracking::routes(security.clone()))
        .merge(tracking::catchup::routes(security.clone()))
        .merge(tracking::home::routes(security.clone()))
        .merge(policy::routes(security.clone()));
    if let Some(google) = &services.google {
        routes = routes
            .merge(auth::google::routes(security.clone(), Arc::clone(google)))
            .merge(auth::identities::routes(security.clone(), google));
    }
    if services.beta {
        routes = routes.merge(beta(services));
    }
    middleware::apply(routes, security)
}

/// Social beta routes (C06–C08). R38 registers them here as their modules land.
fn beta(_services: &Services) -> Router {
    Router::new()
}

/// Runs the outbox mailer, metadata refresher and OAuth flow cleanup until `stop` becomes true.
pub async fn run_workers(services: &Services, mail: MailConfig, stop: watch::Receiver<bool>) {
    let pool = services.security.pool.clone();
    let composer = AuthMail::new(
        pool.clone(),
        services.security.config.public_origin.clone(),
        mail.from,
    );
    let mailer = Worker::new(pool.clone(), mail.transport, composer);
    let refresher = Arc::new(Refresher::new(
        pool.clone(),
        Arc::clone(&services.provider),
        SystemClock,
    ));
    tokio::join!(
        mailer.run(stopped(stop.clone())),
        refresher.run(stopped(stop.clone())),
        purge_oauth_flows(pool, stopped(stop)),
    );
}

/// Resolves once `stop` holds true (or its sender is gone).
pub async fn stopped(mut stop: watch::Receiver<bool>) {
    let _ = stop.wait_for(|stop| *stop).await;
}

async fn purge_oauth_flows(pool: PgPool, shutdown: impl Future<Output = ()>) {
    let mut ticks = tokio::time::interval(Duration::from_secs(60 * 60));
    tokio::pin!(shutdown);
    while tokio::select! {
        () = &mut shutdown => false,
        _ = ticks.tick() => true,
    } {
        if let Err(error) = auth::google::purge_expired(&pool).await {
            tracing::warn!(
                category = crate::failure_kind(&error),
                "oauth flow cleanup failed"
            );
        }
    }
}
