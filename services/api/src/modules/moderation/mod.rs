//! Comment moderation (C07, C08), relative to `/api/v1`. [`reporting`] lets participants report
//! comments. Integration (R38) mounts [`routes`] only while the beta flag is on.

pub mod reporting;

use axum::Router;

use crate::middleware::Security;

/// Every R31 moderation route, for the beta router.
pub fn routes(security: Security) -> Router {
    reporting::routes(security)
}
