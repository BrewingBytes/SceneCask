//! Social beta (C06), relative to `/api/v1`. [`people`] serves search, profiles and visible
//! libraries; [`follows`] serves follow requests, approvals and follower lists; [`privacy`]
//! switches visibility and [`blocks`] manages blocks. Integration (R38)
//! mounts [`routes`] only while the beta flag is on, so every route here is 404 otherwise. Every
//! read and write rechecks the privacy policy in [`crate::domain::access`] against current edges.

pub mod blocks;
pub mod follows;
pub mod people;
pub mod privacy;

use axum::Router;

use crate::middleware::Security;

/// Every R25 and R26 route, for the beta router.
pub fn routes(security: Security) -> Router {
    Router::new()
        .merge(people::routes(security.clone()))
        .merge(follows::routes(security.clone()))
        .merge(privacy::routes(security.clone()))
        .merge(blocks::routes(security))
}
