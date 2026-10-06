use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use scenecask_api::{database_pool, router};
use tower::ServiceExt;
use tracing_test::traced_test;

#[tokio::test]
async fn health_checks_real_postgres_and_redacts_failures() {
    let url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL required: start dev services before running integration tests");
    let pool = database_pool(&url).unwrap();
    let app = router(pool.clone());
    for (path, expected) in [("/health/live", "live"), ("/health/ready", "ready")] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.unwrap(),
            format!("{{\"status\":\"{expected}\"}}")
        );
    }
    pool.close().await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap(),
        "{\"status\":\"database_unavailable\"}"
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
#[traced_test]
async fn unreachable_database_does_not_prevent_liveness() {
    let app = router(
        database_pool("postgres://private-user:private-password@127.0.0.1:1/private-db").unwrap(),
    );
    for (path, status) in [
        ("/health/live", StatusCode::OK),
        ("/health/ready", StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("private"));
    }
    assert!(logs_contain("readiness check failed"));
    assert!(logs_contain("reason=\"pool_timeout\""));
    assert!(!logs_contain("private"));
}

#[test]
fn startup_errors_do_not_echo_configuration() {
    let binary = env!("CARGO_BIN_EXE_scenecask-api");
    let missing = std::process::Command::new(binary)
        .env_remove("DATABASE_URL")
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert_eq!(
        String::from_utf8(missing.stderr).unwrap().trim(),
        "DATABASE_URL is required"
    );
    let invalid = std::process::Command::new(binary)
        .env("DATABASE_URL", "private-secret")
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert_eq!(
        String::from_utf8(invalid.stderr).unwrap().trim(),
        "DATABASE_URL must be a valid PostgreSQL connection URL"
    );
}
