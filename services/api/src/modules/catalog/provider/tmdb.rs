//! Server-only TMDB adapter. Errors discard request URLs, credentials and provider bodies.
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{Datelike, NaiveDate};
use reqwest::{
    Client, Url,
    header::{AUTHORIZATION, HeaderValue, RETRY_AFTER},
};
use serde::de::DeserializeOwned;

use super::*;

const MAX_BODY: usize = 4 * 1024 * 1024;
const METADATA_TTL: Duration = Duration::from_secs(24 * 60 * 60);

pub const ATTRIBUTION: &str =
    "This product uses the TMDB API but is not endorsed or certified by TMDB.";
pub const ATTRIBUTION_URL: &str = "https://www.themoviedb.org/";
/// Consumers must use an approved TMDB logo alongside the attribution (D04).
pub const ATTRIBUTION_GUIDE: &str = "https://developer.themoviedb.org/docs/faq";

pub struct Tmdb {
    client: Client,
    base: Url,
    metadata: Mutex<Option<(Instant, Arc<Metadata>)>>,
}

impl Tmdb {
    /// Use the API read-access bearer token, held only on the backend.
    pub fn new(token: &str) -> Result<Self, ProviderError> {
        Self::build(
            token,
            "https://api.themoviedb.org/3/",
            Duration::from_secs(5),
        )
    }
    fn build(token: &str, base: &str, timeout: Duration) -> Result<Self, ProviderError> {
        if token.trim().is_empty() {
            return Err(ProviderError::Unavailable);
        }
        let mut auth = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| ProviderError::Unavailable)?;
        auth.set_sensitive(true);
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(AUTHORIZATION, auth);
        let client = Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(2))
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| ProviderError::Unavailable)?;
        Ok(Self {
            client,
            base: Url::parse(base).map_err(|_| ProviderError::Unavailable)?,
            metadata: Mutex::new(None),
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, ProviderError> {
        let url = self
            .base
            .join(path)
            .map_err(|_| ProviderError::InvalidData)?;
        for attempt in 0..3 {
            let response = self.client.get(url.clone()).query(query).send().await;
            match response {
                Ok(mut response) if response.status().is_success() => {
                    if response
                        .content_length()
                        .is_some_and(|n| n > MAX_BODY as u64)
                    {
                        return Err(ProviderError::InvalidData);
                    }
                    let mut body = Vec::new();
                    while let Some(chunk) = response
                        .chunk()
                        .await
                        .map_err(|_| ProviderError::Unavailable)?
                    {
                        if body.len() + chunk.len() > MAX_BODY {
                            return Err(ProviderError::InvalidData);
                        }
                        body.extend_from_slice(&chunk);
                    }
                    return serde_json::from_slice(&body).map_err(|_| ProviderError::InvalidData);
                }
                Ok(response) => {
                    if attempt == 2
                        || !(response.status().as_u16() == 429
                            || response.status().is_server_error())
                    {
                        return Err(ProviderError::Unavailable);
                    }
                    let delay = match response.headers().get(RETRY_AFTER) {
                        Some(value) => retry_delay(value)?,
                        None => backoff(attempt),
                    };
                    // Drop the body unread: it may contain protected content or credentials.
                    drop(response);
                    tokio::time::sleep(delay).await;
                }
                Err(_) if attempt < 2 => tokio::time::sleep(backoff(attempt)).await,
                Err(_) => return Err(ProviderError::Unavailable),
            }
        }
        Err(ProviderError::Unavailable)
    }

    async fn metadata(&self) -> Result<Arc<Metadata>, ProviderError> {
        {
            let cache = self.metadata.lock().await;
            if let Some((_, metadata)) = cache
                .as_ref()
                .filter(|(expires, _)| *expires > Instant::now())
            {
                return Ok(Arc::clone(metadata));
            }
        }
        let config: Configuration = self.get("configuration", &[]).await?;
        let base =
            Url::parse(&config.images.secure_base_url).map_err(|_| ProviderError::InvalidData)?;
        if base.scheme() != "https"
            || base.host_str() != Some("image.tmdb.org")
            || base.path() != "/t/p/"
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.port().is_some()
        {
            return Err(ProviderError::InvalidData);
        }
        let size = if config.images.poster_sizes.iter().any(|s| s == "w500") {
            "w500"
        } else if config.images.poster_sizes.iter().any(|s| s == "original") {
            "original"
        } else {
            return Err(ProviderError::InvalidData);
        };
        let genres: Genres = self
            .get("genre/tv/list", &[("language", "en-US".into())])
            .await?;
        let metadata = Arc::new(Metadata {
            poster_base: format!("{}{size}", base.as_str()),
            genres: genres.genres.into_iter().map(|g| (g.id, g.name)).collect(),
        });
        *self.metadata.lock().await = Some((Instant::now() + METADATA_TTL, Arc::clone(&metadata)));
        Ok(metadata)
    }
}

fn backoff(attempt: u32) -> Duration {
    let mut random = [0];
    let _ = getrandom::fill(&mut random);
    Duration::from_millis((100 << attempt) + u64::from(random[0] % 100))
}

fn retry_delay(value: &HeaderValue) -> Result<Duration, ProviderError> {
    let text = value.to_str().map_err(|_| ProviderError::Unavailable)?;
    let seconds = text
        .parse::<u64>()
        .ok()
        .or_else(|| {
            let date = chrono::DateTime::parse_from_rfc2822(text).ok()?.timestamp();
            let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
            Some((date as i128 - now as i128).max(0) as u64)
        })
        .ok_or(ProviderError::Unavailable)?;
    // Long provider cooldowns fail immediately; never retry earlier than Retry-After.
    if seconds > 2 {
        return Err(ProviderError::Unavailable);
    }
    Ok(Duration::from_secs(seconds))
}

struct Metadata {
    poster_base: String,
    genres: BTreeMap<i64, String>,
}
impl Metadata {
    fn poster(&self, path: Option<&str>) -> Option<String> {
        let path = image_path(path)?;
        Some(format!("{}{path}", self.poster_base))
    }
}

fn image_path(path: Option<&str>) -> Option<String> {
    let path = path?;
    if path.len() < 2
        || path.len() > 255
        || !path.starts_with('/')
        || path.contains("..")
        || !path[1..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return None;
    }
    Some(path.to_owned())
}
fn date(value: Option<&str>) -> Result<Option<NaiveDate>, ProviderError> {
    match value.filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) => NaiveDate::parse_from_str(v, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| ProviderError::InvalidData),
    }
}
fn year(value: Option<&str>) -> Result<Option<i16>, ProviderError> {
    Ok(date(value)?
        .map(|d| d.year())
        .filter(|y| (1900..=2999).contains(y))
        .map(|y| y as i16))
}
fn text(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

impl TvProvider for Tmdb {
    async fn search(&self, query: &str, page: u16) -> Result<SearchResults, ProviderError> {
        let metadata = self.metadata().await?;
        let response: RawSearch = self
            .get(
                "search/tv",
                &[
                    ("query", query.to_owned()),
                    ("page", page.to_string()),
                    ("language", "en-US".into()),
                    ("include_adult", "false".into()),
                ],
            )
            .await?;
        if response.page != page {
            return Err(ProviderError::InvalidData);
        }
        let mut items = Vec::with_capacity(response.results.len());
        // One unusable result must not hide the rest; skip it and keep a malformed year null.
        // Each result is parsed separately so a null or missing field skips only that result.
        for show in response.results {
            let Ok(show) = serde_json::from_value::<RawSearchShow>(show) else {
                continue;
            };
            if show.id <= 0 || show.name.trim().is_empty() {
                continue;
            }
            items.push(SearchShow {
                provider: "tmdb",
                provider_id: show.id,
                title: show.name,
                year: year(show.first_air_date.as_deref()).ok().flatten(),
                genres: show
                    .genre_ids
                    .iter()
                    .filter_map(|id| metadata.genres.get(id).cloned())
                    .collect(),
                poster_url: metadata.poster(show.poster_path.as_deref()),
            });
        }
        Ok(SearchResults {
            items,
            page,
            total_pages: response.total_pages,
        })
    }

    async fn catalog(&self, provider_id: i64) -> Result<Catalog, ProviderError> {
        if provider_id <= 0 {
            return Err(ProviderError::InvalidData);
        }
        let show: RawShow = self
            .get(
                &format!("tv/{provider_id}"),
                &[("language", "en-US".into())],
            )
            .await?;
        if show.id != provider_id
            || show.seasons.len() > 1000
            || show.seasons.iter().filter(|s| s.season_number > 0).count() != show.number_of_seasons
        {
            return Err(ProviderError::InvalidData);
        }
        let mut seasons = Vec::with_capacity(show.seasons.len());
        let mut regular_count = 0;
        for summary in show.seasons {
            let season: RawSeason = self
                .get(
                    &format!("tv/{provider_id}/season/{}", summary.season_number),
                    &[("language", "en-US".into())],
                )
                .await?;
            if season.id != summary.id
                || season.season_number != summary.season_number
                || season.episodes.len() != summary.episode_count
            {
                return Err(ProviderError::InvalidData);
            }
            let mut episodes = Vec::with_capacity(season.episodes.len());
            for episode in season.episodes {
                if episode.season_number != season.season_number {
                    return Err(ProviderError::InvalidData);
                }
                episodes.push(Episode {
                    provider_id: episode.id,
                    number: episode.episode_number,
                    title: text(episode.name),
                    overview: text(episode.overview),
                    still_path: image_path(episode.still_path.as_deref()),
                    air_date: date(episode.air_date.as_deref())?,
                });
            }
            if summary.season_number > 0 {
                regular_count += episodes.len();
            }
            seasons.push(Season {
                provider_id: season.id,
                number: season.season_number,
                episodes,
            });
        }
        if regular_count != show.number_of_episodes {
            return Err(ProviderError::InvalidData);
        }
        let catalog = Catalog {
            provider_id,
            title: show.name,
            year: year(show.first_air_date.as_deref())?,
            genres: show.genres.into_iter().map(|g| g.name).collect(),
            synopsis: text(show.overview),
            poster_path: image_path(show.poster_path.as_deref()),
            status: match show.status.as_str() {
                "Returning Series" | "In Production" | "Planned" | "Pilot" => ShowStatus::Returning,
                "Ended" => ShowStatus::Ended,
                "Canceled" => ShowStatus::Canceled,
                _ => ShowStatus::Unknown,
            },
            seasons,
        };
        catalog.validate(provider_id)?;
        Ok(catalog)
    }
}

#[derive(Deserialize)]
struct Configuration {
    images: Images,
}
#[derive(Deserialize)]
struct Images {
    secure_base_url: String,
    poster_sizes: Vec<String>,
}
#[derive(Deserialize)]
struct Genre {
    id: i64,
    name: String,
}
#[derive(Deserialize)]
struct Genres {
    genres: Vec<Genre>,
}
#[derive(Deserialize)]
struct RawSearch {
    page: u16,
    total_pages: u32,
    results: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
struct RawSearchShow {
    id: i64,
    name: String,
    first_air_date: Option<String>,
    genre_ids: Vec<i64>,
    poster_path: Option<String>,
}
#[derive(Deserialize)]
struct RawShow {
    id: i64,
    name: String,
    first_air_date: Option<String>,
    genres: Vec<Genre>,
    overview: Option<String>,
    poster_path: Option<String>,
    status: String,
    number_of_seasons: usize,
    number_of_episodes: usize,
    seasons: Vec<RawSeasonSummary>,
}
#[derive(Deserialize)]
struct RawSeasonSummary {
    id: i64,
    season_number: i32,
    episode_count: usize,
}
#[derive(Deserialize)]
struct RawSeason {
    id: i64,
    season_number: i32,
    episodes: Vec<RawEpisode>,
}
#[derive(Deserialize)]
struct RawEpisode {
    id: i64,
    season_number: i32,
    episode_number: i32,
    name: Option<String>,
    overview: Option<String>,
    still_path: Option<String>,
    air_date: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::{
        Router,
        extract::{Request, State},
        http::StatusCode,
        response::IntoResponse,
    };

    use super::*;

    const FIXTURE: &str = include_str!("../../../../tests/catalog_fixtures/search.json");
    const SHOW: &str = include_str!("../../../../tests/catalog_fixtures/show.json");
    const S0: &str = include_str!("../../../../tests/catalog_fixtures/season0.json");
    const S1: &str = include_str!("../../../../tests/catalog_fixtures/season1.json");

    #[derive(Clone)]
    struct Fixture {
        mode: &'static str,
        calls: Arc<AtomicUsize>,
        paths: Arc<Mutex<Vec<String>>>,
    }
    async fn serve(State(fixture): State<Fixture>, request: Request) -> axum::response::Response {
        fixture.calls.fetch_add(1, Ordering::SeqCst);
        assert!(
            request
                .headers()
                .get(AUTHORIZATION)
                .is_some_and(|v| v == "Bearer fixture-only")
        );
        fixture.paths.lock().await.push(request.uri().to_string());
        if fixture.mode == "timeout" {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        match fixture.mode {
            "429" => {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    [(RETRY_AFTER, "0")],
                    "protected payload /protected-image.jpg",
                )
                    .into_response();
            }
            "cooldown" => {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    [(RETRY_AFTER, "10")],
                    "protected payload",
                )
                    .into_response();
            }
            "500" => {
                return (StatusCode::INTERNAL_SERVER_ERROR, "protected payload").into_response();
            }
            "404" => return (StatusCode::NOT_FOUND, "protected payload").into_response(),
            "redirect" => {
                return (
                    StatusCode::FOUND,
                    [(reqwest::header::LOCATION, "/secret")],
                    "protected payload",
                )
                    .into_response();
            }
            "malformed" => return "{private:not JSON}".into_response(),
            "huge" => return "x".repeat(MAX_BODY + 1).into_response(),
            _ => {}
        }
        let body = match request.uri().path() {
            "/3/configuration" => {
                r#"{"images":{"secure_base_url":"https://image.tmdb.org/t/p/","poster_sizes":["w500","original"]}}"#
            }
            "/3/genre/tv/list" => {
                r#"{"genres":[{"id":18,"name":"Drama"},{"id":35,"name":"Comedy"}]}"#
            }
            "/3/search/tv" => FIXTURE,
            "/3/tv/123" => SHOW,
            "/3/tv/123/season/0" => S0,
            "/3/tv/123/season/1" if fixture.mode == "incomplete" => {
                r#"{"id":1001,"season_number":1,"episodes":[]}"#
            }
            "/3/tv/123/season/1" => S1,
            _ => panic!("unexpected provider endpoint"),
        };
        ([(reqwest::header::CONTENT_TYPE, "application/json")], body).into_response()
    }

    async fn fixture(mode: &'static str) -> (Tmdb, Fixture, tokio::task::JoinHandle<()>) {
        let state = Fixture {
            mode,
            calls: Arc::new(AtomicUsize::new(0)),
            paths: Arc::new(Mutex::new(Vec::new())),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/3/", listener.local_addr().unwrap());
        let app = Router::new().fallback(serve).with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let timeout = if mode == "timeout" {
            Duration::from_millis(20)
        } else {
            Duration::from_secs(1)
        };
        let tmdb = Tmdb::build("fixture-only", &base, timeout).unwrap();
        (tmdb, state, task)
    }

    #[tokio::test]
    async fn maps_tv_only_search_and_caches_configuration() {
        let (tmdb, state, task) = fixture("ok").await;
        let results = tmdb.search("Hollow & orchard", 1).await.unwrap();
        assert_eq!(results.items[0].year, Some(2024));
        assert_eq!(results.items[1].year, Some(2010));
        assert_eq!(results.items[0].genres, ["Drama"]);
        assert_eq!(results.items[1].genres, ["Comedy"]);
        assert_eq!(
            results.items[0].poster_url.as_deref(),
            Some("https://image.tmdb.org/t/p/w500/orchard.jpg")
        );
        assert!(results.items[1].poster_url.is_none());
        assert!(results.items[2].poster_url.is_none());
        assert!(results.items[2].year.is_none());
        // Unusable results (id 0, empty or null name, missing genre_ids) are skipped without
        // failing the page; the malformed date becomes a null year.
        assert_eq!(results.items.len(), 4);
        assert_eq!(results.items[3].provider_id, 126);
        assert!(results.items[3].year.is_none());
        let serialized = serde_json::to_string(&results).unwrap();
        for value in ["overview", "private_unknown", "fixture-only", "untrusted"] {
            assert!(!serialized.contains(value));
        }
        tmdb.search("Hollow & orchard", 1).await.unwrap();
        let paths = state.paths.lock().await;
        assert_eq!(paths.iter().filter(|p| *p == "/3/configuration").count(), 1);
        assert!(paths.iter().any(|p| p.starts_with("/3/search/tv?")
            && p.contains("query=Hollow+%26+orchard")
            && p.contains("include_adult=false")));
        assert!(!paths.iter().any(|p| p.contains("api_key")));
        task.abort();
    }

    #[tokio::test]
    async fn imports_every_season_including_specials_and_maps_null_dates() {
        let (tmdb, _, task) = fixture("ok").await;
        let show = tmdb.catalog(123).await.unwrap();
        assert_eq!(show.seasons.len(), 2);
        assert_eq!(show.status.as_str(), "returning");
        assert!(show.poster_path.is_none());
        assert!(show.seasons[1].episodes[0].air_date.is_none());
        assert_eq!(show.seasons[1].episodes[1].air_date.unwrap().year(), 2999);
        assert!(show.seasons[1].episodes[1].overview.is_none());
        task.abort();
        let (tmdb, _, task) = fixture("incomplete").await;
        assert!(matches!(
            tmdb.catalog(123).await,
            Err(ProviderError::InvalidData)
        ));
        task.abort();
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn bounds_transport_failures_and_redacts_payloads() {
        for (mode, expected) in [
            ("429", 3),
            ("500", 3),
            ("timeout", 3),
            ("cooldown", 1),
            ("404", 1),
            ("redirect", 1),
            ("malformed", 1),
            ("huge", 1),
        ] {
            let (tmdb, state, task) = fixture(mode).await;
            let error = tmdb.get::<RawSearch>("search/tv", &[]).await.err().unwrap();
            let api: ApiError = error.into();
            assert_eq!(api.status(), StatusCode::BAD_GATEWAY);
            let response = api.into_response();
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            let body = String::from_utf8(bytes.to_vec()).unwrap();
            for protected in ["fixture-only", "protected", "private", "/secret"] {
                assert!(!body.contains(protected));
            }
            assert_eq!(state.calls.load(Ordering::SeqCst), expected, "mode: {mode}");
            task.abort();
        }
        assert!(!logs_contain("protected payload"));
        assert!(!logs_contain("fixture-only"));
    }

    #[test]
    fn dates_image_paths_and_retry_after_are_safe() {
        assert_eq!(date(None).unwrap(), None);
        assert_eq!(date(Some("")).unwrap(), None);
        assert!(date(Some("2024-02-30")).is_err());
        for path in [
            "//evil.example/a",
            "/../a.jpg",
            "/image.jpg?api_key=secret",
            "/a%2Fb.jpg",
        ] {
            assert!(image_path(Some(path)).is_none());
        }
        assert_eq!(
            retry_delay(&HeaderValue::from_static("2")).unwrap(),
            Duration::from_secs(2)
        );
        assert!(retry_delay(&HeaderValue::from_static("3")).is_err());
        assert_eq!(
            retry_delay(&HeaderValue::from_static("Wed, 21 Oct 2015 07:28:00 GMT")).unwrap(),
            Duration::ZERO
        );
        assert!(retry_delay(&HeaderValue::from_static("unknown")).is_err());
        assert!(Tmdb::new("").is_err());
    }
}
