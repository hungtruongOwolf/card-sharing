//! T-70: uploads stream into quarantine; size, magic bytes and header dimensions are checked
//! before anything is stored; the state route is owner-only and never returns a URL.

use std::time::{Duration, Instant};

use crate::common::*;
use crate::social_support::*;
use axum::body::Body;
use http::{Request, Response, StatusCode};
use media::Bucket;
use serde_json::Value;

/// A PNG signature and IHDR header claiming `width` x `height` (enough for the upload checks;
/// decoding is the worker's job).
pub fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

/// A unique small "PNG" (distinct bytes, so uploads do not dedupe by accident).
pub fn unique_png() -> Vec<u8> {
    let mut bytes = png_header(64, 64);
    bytes.extend_from_slice(uuid::Uuid::now_v7().as_bytes());
    bytes
}

pub fn upload_request(
    token: Option<&str>,
    kind: &str,
    content_type: Option<&str>,
    bytes: Vec<u8>,
    declare_length: bool,
) -> Request<Body> {
    let mut builder = Request::post(format!("/v1/media?kind={kind}"));
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    if declare_length {
        builder = builder.header("content-length", bytes.len());
    }
    builder.body(Body::from(bytes)).unwrap()
}

pub async fn upload(env: &TestEnv, user: &User, kind: &str, bytes: Vec<u8>) -> Response<Body> {
    send(
        &env.router,
        upload_request(Some(&user.token), kind, Some("image/png"), bytes, true),
    )
    .await
}

#[tokio::test]
async fn upload_lands_in_quarantine_and_dedupes() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let bytes = unique_png();
    let response = upload(&env, &user, "card_render", bytes.clone()).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = json_body(response).await;
    let media = &body["media"];
    assert_eq!(media["state"], "uploaded");
    assert_eq!(media["kind"], "card_render");
    assert_eq!(
        (media["width"].as_i64(), media["height"].as_i64()),
        (Some(64), Some(64))
    );
    assert_no_url(&body);
    assert_eq!(env.media_store.count(Bucket::Quarantine), 1);
    assert_eq!(env.media_store.count(Bucket::Clean), 0);
    let id = media["id"].as_str().unwrap().to_owned();
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.jobs WHERE kind = 'media.process' AND payload->>'media_id' = $1",
    )
    .bind(&id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(queued, 1);

    // Same bytes, same kind: the existing upload (no second object, no second job).
    let again = upload(&env, &user, "card_render", bytes.clone()).await;
    assert_eq!(again.status(), StatusCode::OK);
    assert_eq!(json_body(again).await["media"]["id"], id.as_str());
    assert_eq!(env.media_store.count(Bucket::Quarantine), 1);
    // Same bytes as another kind are another upload.
    let mask = upload(&env, &user, "mask", bytes).await;
    assert_eq!(mask.status(), StatusCode::CREATED);
}

fn assert_no_url(body: &Value) {
    let text = body.to_string();
    for needle in ["url", "storage_key", "http"] {
        assert!(!text.contains(needle), "{needle} leaked in {text}");
    }
}

#[tokio::test]
async fn oversized_uploads_are_refused_with_413() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let mut big = png_header(64, 64);
    big.resize(11 * 1024 * 1024, 0);
    for declare_length in [true, false] {
        let response = send(
            &env.router,
            upload_request(
                Some(&user.token),
                "card_render",
                Some("image/png"),
                big.clone(),
                declare_length,
            ),
        )
        .await;
        assert_problem(response, StatusCode::PAYLOAD_TOO_LARGE, "MEDIA_TOO_LARGE").await;
    }
    assert_eq!(env.media_store.count(Bucket::Quarantine), 0);
}

#[tokio::test]
async fn html_renamed_png_is_415() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let html = b"<!doctype html><html><script>alert(document.cookie)</script></html>".to_vec();
    for content_type in [Some("image/png"), Some("application/octet-stream")] {
        let response = send(
            &env.router,
            upload_request(
                Some(&user.token),
                "card_render",
                content_type,
                html.clone(),
                true,
            ),
        )
        .await;
        assert_problem(
            response,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "MEDIA_TYPE_UNSUPPORTED",
        )
        .await;
    }
    // A declared type that disagrees with the magic bytes, or a non-image content type.
    for content_type in [Some("image/jpeg"), Some("text/html"), None] {
        let response = send(
            &env.router,
            upload_request(
                Some(&user.token),
                "card_render",
                content_type,
                unique_png(),
                true,
            ),
        )
        .await;
        assert_problem(
            response,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "MEDIA_TYPE_UNSUPPORTED",
        )
        .await;
    }
    assert_eq!(env.media_store.count(Bucket::Quarantine), 0);
}

#[tokio::test]
async fn huge_header_is_400_fast() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let mut bytes = png_header(50_000, 50_000);
    bytes.resize(4096, 0);
    let started = Instant::now();
    let response = upload(&env, &user, "card_render", bytes).await;
    let elapsed = started.elapsed();
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    assert!(elapsed < Duration::from_millis(50), "took {elapsed:?}");
    assert_eq!(env.media_store.count(Bucket::Quarantine), 0);
}

#[tokio::test]
async fn bad_kind_and_query_are_400() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    for kind in ["thumbnail", "card_render&owner_id=x"] {
        let response = upload(&env, &user, kind, unique_png()).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
}

#[tokio::test]
async fn media_state_is_owner_only_and_404_otherwise() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let other = adult(&env).await;
    let response = upload(&env, &owner, "card_render", unique_png()).await;
    let id = json_body(response).await["media"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let mine = call(&env, "GET", &format!("/v1/media/{id}"), &owner, None).await;
    assert_eq!(mine.status(), StatusCode::OK);
    let body = json_body(mine).await;
    assert_eq!(body["media"]["state"], "uploaded");
    assert_no_url(&body);

    let theirs = call(&env, "GET", &format!("/v1/media/{id}"), &other, None).await;
    assert_eq!(theirs.status(), StatusCode::NOT_FOUND);
    let denied = body_bytes(theirs).await;
    let missing = call(
        &env,
        "GET",
        "/v1/media/0192f0c1-7c1e-7a3b-9d2e-000000000000",
        &other,
        None,
    )
    .await;
    assert_eq!(body_bytes(missing).await, denied);
    let malformed = call(&env, "GET", "/v1/media/not-a-uuid", &other, None).await;
    assert_eq!(malformed.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn guests_cannot_upload() {
    let response = send(
        &offline_app(),
        upload_request(None, "card_render", Some("image/png"), unique_png(), true),
    )
    .await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}
