//! R15 `PATCH /me` (C04), relative to `/api/v1`. Sets the display name and the initial handle.
//! The handle is unique and fixed once chosen; role, visibility and email are not writable here,
//! and unknown request keys (such as `role`) are rejected.

use axum::{Json, Router, extract::State, routing::patch};
use serde::Deserialize;

use crate::{
    error::{ApiError, ErrorCode},
    middleware::{Security, json::ApiJson},
    modules::auth::session::{UserDto, VerifiedUser},
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/me", patch(update_profile))
        .with_state(security)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProfileRequest {
    display_name: String,
    handle: String,
}

const HANDLE_RULE: &str = "Use 3–30 lowercase letters, numbers or underscores.";

/// A trimmed name of 1–80 characters without control characters.
fn valid_display_name(name: &str) -> Option<&str> {
    let name = name.trim();
    ((1..=80).contains(&name.chars().count()) && !name.chars().any(char::is_control))
        .then_some(name)
}

fn valid_handle(handle: &str) -> bool {
    (3..=30).contains(&handle.len())
        && handle
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

async fn update_profile(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    ApiJson(request): ApiJson<ProfileRequest>,
) -> Result<Json<UserDto>, ApiError> {
    let display_name = valid_display_name(&request.display_name);
    let handle_ok = valid_handle(&request.handle);
    if display_name.is_none() || !handle_ok {
        let mut error = ApiError::new(ErrorCode::ValidationError);
        if display_name.is_none() {
            error = error.with_field("displayName", "Use 1–80 characters.");
        }
        if !handle_ok {
            error = error.with_field("handle", HANDLE_RULE);
        }
        return Err(error);
    }
    // Setting the same handle again is allowed; a different one only while none is chosen.
    let updated: Option<(Option<String>, Option<String>, String)> = sqlx::query_as(
        "UPDATE users SET display_name = $2, handle = $3
         WHERE id = $1 AND (handle IS NULL OR handle = $3)
         RETURNING display_name, handle, visibility",
    )
    .bind(current.user.id)
    .bind(display_name)
    .bind(&request.handle)
    .fetch_optional(&security.pool)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .is_some_and(|e| e.is_unique_violation())
        {
            ApiError::new(ErrorCode::HandleTaken).with_field("handle", "That handle is taken.")
        } else {
            error.into()
        }
    })?;
    let (display_name, handle, visibility) = updated.ok_or_else(|| {
        ApiError::new(ErrorCode::ValidationError)
            .with_field("handle", "Your handle can't be changed.")
    })?;
    let mut user = current.user;
    user.display_name = display_name;
    user.handle = handle;
    user.visibility = visibility;
    Ok(Json(UserDto::from(&user)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_rule_matches_the_schema() {
        for valid in ["ana", "ana_r", "a1_", &"a".repeat(30)] {
            assert!(valid_handle(valid), "{valid}");
        }
        for invalid in ["an", "Ana", "ana-r", "ana r", "ánа", "", &"a".repeat(31)] {
            assert!(!valid_handle(invalid), "{invalid}");
        }
    }

    #[test]
    fn display_name_is_trimmed_and_bounded_by_characters() {
        assert_eq!(valid_display_name("  Ana  "), Some("Ana"));
        assert_eq!(valid_display_name(&"é".repeat(80)).map(str::len), Some(160));
        for invalid in ["", "   ", "A\u{0}na", &"é".repeat(81)] {
            assert_eq!(valid_display_name(invalid), None, "{invalid:?}");
        }
    }
}
