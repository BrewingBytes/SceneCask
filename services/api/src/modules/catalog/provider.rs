//! TV-only metadata boundary. Only search DTOs may be serialized to clients here.
pub mod tmdb;

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::Arc,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::{sync::Mutex, time::Instant};

use crate::error::{ApiError, ErrorCode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderError {
    Unavailable,
    InvalidData,
    /// The provider asked callers to wait this long (429 Retry-After) before trying again.
    RateLimited(Duration),
}

impl From<ProviderError> for ApiError {
    fn from(_: ProviderError) -> Self {
        Self::new(ErrorCode::ProviderUnavailable)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchShow {
    pub provider: &'static str,
    pub provider_id: i64,
    pub title: String,
    pub year: Option<i16>,
    pub genres: Vec<String>,
    pub poster_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub items: Vec<SearchShow>,
    pub page: u16,
    pub total_pages: u32,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "first_page")]
    pub page: u16,
}
fn first_page() -> u16 {
    1
}

/// Raw query string parameters, so a missing `q` or non-numeric `page` is a 422 field error
/// like any other invalid value rather than a 400 request-format error.
#[derive(Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub page: Option<String>,
}

impl TryFrom<SearchParams> for SearchQuery {
    type Error = ApiError;

    fn try_from(params: SearchParams) -> Result<Self, ApiError> {
        let page = match params.page {
            None => first_page(),
            Some(page) => page.parse().map_err(|_| page_error())?,
        };
        Ok(Self {
            q: params.q.unwrap_or_default(),
            page,
        })
    }
}

fn page_error() -> ApiError {
    ApiError::new(ErrorCode::ValidationError).with_field("page", "Use a page from 1 to 500.")
}

impl SearchQuery {
    pub fn validate(&self) -> Result<&str, ApiError> {
        let q = self.q.trim();
        if !(2..=100).contains(&q.chars().count()) {
            return Err(
                ApiError::new(ErrorCode::ValidationError).with_field("q", "Use 2–100 characters.")
            );
        }
        if !(1..=500).contains(&self.page) {
            return Err(page_error());
        }
        Ok(q)
    }
}

/// Internal metadata: never serialize or log episode titles, overviews or image paths.
#[derive(Clone)]
pub struct Catalog {
    pub provider_id: i64,
    pub title: String,
    pub year: Option<i16>,
    pub genres: Vec<String>,
    pub synopsis: Option<String>,
    pub poster_path: Option<String>,
    pub status: ShowStatus,
    pub seasons: Vec<Season>,
}

#[derive(Clone, Copy)]
pub enum ShowStatus {
    Returning,
    Ended,
    Canceled,
    Unknown,
}
impl ShowStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Returning => "returning",
            Self::Ended => "ended",
            Self::Canceled => "canceled",
            Self::Unknown => "unknown",
        }
    }
}
#[derive(Clone)]
pub struct Season {
    pub provider_id: i64,
    pub number: i32,
    pub episodes: Vec<Episode>,
}
#[derive(Clone)]
pub struct Episode {
    pub provider_id: i64,
    pub number: i32,
    pub title: Option<String>,
    pub overview: Option<String>,
    pub still_path: Option<String>,
    pub air_date: Option<chrono::NaiveDate>,
}

impl Catalog {
    /// Checks identity and ordering before any database writes, including alternate providers.
    pub fn validate(&self, expected_id: i64) -> Result<(), ProviderError> {
        if self.provider_id != expected_id
            || expected_id <= 0
            || self.title.trim().is_empty()
            || self.year.is_some_and(|y| !(1900..=2999).contains(&y))
        {
            return Err(ProviderError::InvalidData);
        }
        let mut season_ids = HashSet::new();
        let mut season_numbers = HashSet::new();
        let mut episode_ids = HashSet::new();
        for season in &self.seasons {
            if season.provider_id <= 0
                || season.number < 0
                || !season_ids.insert(season.provider_id)
                || !season_numbers.insert(season.number)
            {
                return Err(ProviderError::InvalidData);
            }
            let mut numbers = HashSet::new();
            for episode in &season.episodes {
                if episode.provider_id <= 0
                    || episode.number < 0
                    || !episode_ids.insert(episode.provider_id)
                    || !numbers.insert(episode.number)
                {
                    return Err(ProviderError::InvalidData);
                }
            }
        }
        Ok(())
    }
}

pub trait TvProvider: Send + Sync {
    fn search(
        &self,
        query: &str,
        page: u16,
    ) -> impl Future<Output = Result<SearchResults, ProviderError>> + Send;
    fn catalog(
        &self,
        provider_id: i64,
    ) -> impl Future<Output = Result<Catalog, ProviderError>> + Send;
}

/// A bounded, server-only 15-minute query cache. Failed requests are never cached.
pub struct Search<P> {
    provider: Arc<P>,
    cache: Mutex<HashMap<(String, u16), (Instant, SearchResults)>>,
}
impl<P: TvProvider> Search<P> {
    pub fn new(provider: Arc<P>) -> Self {
        Self {
            provider,
            cache: Mutex::new(HashMap::new()),
        }
    }
    pub async fn search(&self, query: &SearchQuery) -> Result<SearchResults, ApiError> {
        let q = query.validate()?;
        let key = (q.to_owned(), query.page);
        {
            let mut cache = self.cache.lock().await;
            cache.retain(|_, (expires, _)| *expires > Instant::now());
            if let Some((_, result)) = cache.get(&key) {
                return Ok(result.clone());
            }
        }
        let result =
            tokio::time::timeout(Duration::from_secs(20), self.provider.search(q, query.page))
                .await
                .map_err(|_| ProviderError::Unavailable)??;
        let mut cache = self.cache.lock().await;
        if cache.len() >= 512
            && let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, (expiry, _))| expiry)
                .map(|(key, _)| key.clone())
        {
            cache.remove(&oldest);
        }
        cache.insert(
            key,
            (Instant::now() + Duration::from_secs(900), result.clone()),
        );
        Ok(result)
    }
}

/// State for the search handler; keep this instance shared across requests to retain the cache.
pub struct SearchState<P> {
    pub search: Arc<Search<P>>,
    pub limiter: crate::middleware::rate_limit::RateLimiter,
    pub rule: crate::middleware::rate_limit::Rule,
}

impl<P> Clone for SearchState<P> {
    fn clone(&self) -> Self {
        Self {
            search: Arc::clone(&self.search),
            limiter: self.limiter.clone(),
            rule: self.rule,
        }
    }
}

/// R24 registers GET /shows/search and applies the existing C03 security middleware.
pub async fn search_handler<P: TvProvider + 'static>(
    verified: crate::modules::auth::session::VerifiedUser,
    axum::extract::State(state): axum::extract::State<SearchState<P>>,
    query: Result<axum::extract::Query<SearchParams>, axum::extract::rejection::QueryRejection>,
) -> Result<axum::Json<SearchResults>, ApiError> {
    state.limiter.check(
        &format!("catalog.search:{}", verified.0.user.id),
        &[state.rule],
    )?;
    let axum::extract::Query(params) = query.map_err(|_| ApiError::malformed())?;
    Ok(axum::Json(state.search.search(&params.try_into()?).await?))
}
