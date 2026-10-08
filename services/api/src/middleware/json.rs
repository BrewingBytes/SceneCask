//! JSON body extractor whose rejections use the C03 envelope and never echo parser messages,
//! which can quote submitted content.

use axum::{
    Json,
    extract::{FromRequest, Request},
};
use serde::{Deserialize, de::DeserializeOwned};

use crate::error::ApiError;

pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(request, state).await?;
        Ok(Self(value))
    }
}

/// `{}`: the request body of bodyless mutations such as logout. Unknown keys are rejected.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyRequest {}
