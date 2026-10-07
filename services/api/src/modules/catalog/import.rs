//! Atomic imports preserve stable provider identity and never delete progress-bearing rows.
use super::provider::{Catalog, ProviderError, TvProvider};
use crate::error::{ApiError, ErrorCode};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportRequest {
    pub provider_id: i64,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub show_id: Uuid,
}

pub struct Importer<P> {
    pool: PgPool,
    provider: Arc<P>,
}
impl<P: TvProvider> Importer<P> {
    pub fn new(pool: PgPool, provider: Arc<P>) -> Self {
        Self { pool, provider }
    }

    pub async fn import(&self, request: &ImportRequest) -> Result<ImportResult, ApiError> {
        if request.provider_id <= 0 {
            return Err(ApiError::new(ErrorCode::ValidationError)
                .with_field("providerId", "Choose a valid TV show."));
        }
        if let Some(id) = complete(&self.pool, request.provider_id, true).await? {
            return Ok(ImportResult { show_id: id });
        }
        match self.refresh(request.provider_id).await {
            // C05: a failed refresh keeps the last good catalog usable; only a first import fails.
            Err(error) if error.code() == ErrorCode::ProviderUnavailable => {
                match complete(&self.pool, request.provider_id, false).await? {
                    Some(id) => Ok(ImportResult { show_id: id }),
                    None => Err(error),
                }
            }
            result => result,
        }
    }

    async fn refresh(&self, provider_id: i64) -> Result<ImportResult, ApiError> {
        // Do not hold a database transaction while waiting on provider I/O.
        let catalog =
            tokio::time::timeout(Duration::from_secs(30), self.provider.catalog(provider_id))
                .await
                .map_err(|_| ProviderError::Unavailable)??;
        catalog.validate(provider_id)?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '5s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL statement_timeout = '10s'")
            .execute(&mut *tx)
            .await?;
        // Cross-process lock also works before a show row exists. Every importer uses this key.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('catalog.import.' || $1::bigint::text, 0))")
            .bind(provider_id).execute(&mut *tx).await?;
        let existing: Option<(Uuid, bool)> = sqlx::query_as(
            "SELECT id, complete_import AND fetched_at > now() - interval '24 hours' FROM shows WHERE tmdb_id = $1 FOR UPDATE")
            .bind(provider_id).fetch_optional(&mut *tx).await?;
        if let Some((id, true)) = existing {
            tx.commit().await?;
            return Ok(ImportResult { show_id: id });
        }
        let id = match existing {
            Some((id, false)) => id,
            _ => sqlx::query_scalar("INSERT INTO shows (tmdb_id, title, fetched_at) VALUES ($1, $2, now()) RETURNING id")
                .bind(catalog.provider_id).bind(&catalog.title).fetch_one(&mut *tx).await?,
        };
        persist(&mut tx, id, &catalog).await?;
        tx.commit().await.map_err(import_database_error)?;
        Ok(ImportResult { show_id: id })
    }
}

/// The show's ID if it has a complete import; `recent_only` also requires a fetch within 24 hours.
async fn complete(
    pool: &PgPool,
    provider_id: i64,
    recent_only: bool,
) -> Result<Option<Uuid>, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM shows WHERE tmdb_id = $1 AND complete_import
         AND (NOT $2 OR fetched_at > now() - interval '24 hours')",
    )
    .bind(provider_id)
    .bind(recent_only)
    .fetch_optional(pool)
    .await?)
}

fn import_database_error(error: sqlx::Error) -> ApiError {
    // Provider identity/number collisions mean this snapshot cannot be imported. Never expose
    // database detail: it can contain protected titles and asset paths.
    if error.as_database_error().is_some_and(|e| {
        e.is_unique_violation()
            || e.is_foreign_key_violation()
            || e.code().is_some_and(|c| c == "23P01" || c == "23514")
    }) {
        ProviderError::InvalidData.into()
    } else {
        error.into()
    }
}

async fn persist(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    catalog: &Catalog,
) -> Result<(), ApiError> {
    sqlx::query("SET CONSTRAINTS seasons_active_show_number_key, episodes_active_season_number_key DEFERRED")
        .execute(&mut **tx).await?;
    // Archive missing rows first; restored rows retain their application IDs. Deferral supports
    // numbering swaps and moves between seasons without deleting any row/history.
    let season_ids: Vec<i64> = catalog.seasons.iter().map(|s| s.provider_id).collect();
    let episode_ids: Vec<i64> = catalog
        .seasons
        .iter()
        .flat_map(|s| s.episodes.iter().map(|e| e.provider_id))
        .collect();
    sqlx::query("UPDATE episodes SET archived_at = now() WHERE show_id = $1 AND archived_at IS NULL AND NOT (tmdb_id = ANY($2))")
        .bind(id).bind(&episode_ids).execute(&mut **tx).await?;
    sqlx::query("UPDATE seasons SET archived_at = now() WHERE show_id = $1 AND archived_at IS NULL AND NOT (tmdb_id = ANY($2))")
        .bind(id).bind(&season_ids).execute(&mut **tx).await?;
    for season in &catalog.seasons {
        let season_id: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO seasons (tmdb_id, show_id, number) VALUES ($1, $2, $3)
             ON CONFLICT (tmdb_id) DO UPDATE SET number = EXCLUDED.number, archived_at = NULL
             WHERE seasons.show_id = EXCLUDED.show_id RETURNING id",
        )
        .bind(season.provider_id)
        .bind(id)
        .bind(season.number)
        .fetch_optional(&mut **tx)
        .await
        .map_err(import_database_error)?;
        let season_id = season_id.ok_or(ProviderError::InvalidData)?;
        for episode in &season.episodes {
            let episode_id: Option<Uuid> = sqlx::query_scalar(
                "INSERT INTO episodes (tmdb_id, show_id, season_id, number, title, overview, still_path, air_date, release_timezone)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL)
                 ON CONFLICT (tmdb_id) DO UPDATE SET season_id = EXCLUDED.season_id, number = EXCLUDED.number,
                 title = EXCLUDED.title, overview = EXCLUDED.overview, still_path = EXCLUDED.still_path,
                 air_date = EXCLUDED.air_date, release_timezone = NULL, archived_at = NULL
                 WHERE episodes.show_id = EXCLUDED.show_id RETURNING id")
                .bind(episode.provider_id).bind(id).bind(season_id).bind(episode.number)
                .bind(&episode.title).bind(&episode.overview).bind(&episode.still_path).bind(episode.air_date)
                .fetch_optional(&mut **tx).await.map_err(import_database_error)?;
            if episode_id.is_none() {
                return Err(ProviderError::InvalidData.into());
            }
        }
    }
    // catalog_revision advances through the 0002 triggers only when seasons/episodes change, so
    // an unchanged refresh keeps catch-up previews valid.
    sqlx::query("UPDATE shows SET title = $2, first_air_year = $3, genres = $4, synopsis = $5, poster_path = $6,
                 status = $7, fetched_at = now(), complete_import = true WHERE id = $1")
        .bind(id).bind(&catalog.title).bind(catalog.year).bind(sqlx::types::Json(&catalog.genres)).bind(&catalog.synopsis)
        .bind(&catalog.poster_path).bind(catalog.status.as_str()).execute(&mut **tx).await.map_err(import_database_error)?;
    Ok(())
}

/// R24 registers POST /shows/import behind the existing origin/CSRF/body/write-limit layers.
pub async fn import_handler<P: TvProvider + 'static>(
    _: crate::modules::auth::session::VerifiedUser,
    axum::extract::State(importer): axum::extract::State<Arc<Importer<P>>>,
    crate::middleware::json::ApiJson(request): crate::middleware::json::ApiJson<ImportRequest>,
) -> Result<axum::Json<ImportResult>, ApiError> {
    Ok(axum::Json(importer.import(&request).await?))
}
