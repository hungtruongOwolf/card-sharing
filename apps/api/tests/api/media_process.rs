//! T-71: `media.process` re-encodes an upload in the sandboxed task. The sanitized image and its
//! derivatives stay in quarantine (state `scanning`) until the content gates pass (here they are
//! unconfigured, so the job asks to be retried); undecodable
//! uploads and decoder timeouts end `failed` without crashing the worker.

use std::time::Duration;

use crate::common::*;
use crate::media::{png_header, upload};
use crate::social_support::*;
use http::StatusCode;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
use media::{Bucket, ProcessSettings};
use uuid::Uuid;

/// A real PNG with unique pixels (no accidental dedupe between tests).
pub fn real_png(width: u32, height: u32) -> Vec<u8> {
    real_png_with(width, height, Rgba([200, 120, 40, 255]))
}

/// A real PNG whose top-left pixel is `marker` (the fake content-safety adapters read it).
pub fn real_png_with(width: u32, height: u32, marker: Rgba<u8>) -> Vec<u8> {
    let salt = Uuid::now_v7();
    let salt = salt.as_bytes();
    let pixels = RgbaImage::from_fn(width, height, |x, y| {
        if x == 0 && y == 0 {
            return marker;
        }
        let i = ((x + y) as usize) % salt.len();
        Rgba([salt[i], (x % 256) as u8, (y % 256) as u8, 255])
    });
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(pixels.as_raw(), width, height, ExtendedColorType::Rgba8)
        .unwrap();
    out
}

pub async fn uploaded_id(env: &TestEnv, user: &User, kind: &str, bytes: Vec<u8>) -> Uuid {
    let response = upload(env, user, kind, bytes).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = json_body(response).await;
    Uuid::parse_str(body["media"]["id"].as_str().unwrap()).unwrap()
}

async fn row(env: &TestEnv, id: Uuid) -> (String, Option<String>, String, bool) {
    sqlx::query_as(
        "SELECT state, failure_code, content_type, processed_at IS NOT NULL
         FROM media.media_objects WHERE id = $1",
    )
    .bind(id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn processing_stores_sanitized_copies_and_keeps_them_quarantined() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let before = env.media_store.count(Bucket::Quarantine);
    let id = uploaded_id(&env, &user, "card_render", real_png(1600, 1200)).await;
    env.state
        .media
        .process_one(id, &ProcessSettings::default())
        .await
        .expect_err("unconfigured gates retry");

    let (state, failure, content_type, processed) = row(&env, id).await;
    assert_eq!(state, "scanning", "nothing is clean before the gates");
    assert_eq!(failure, None);
    assert_eq!(content_type, "image/webp");
    assert!(processed);
    let variants: Vec<(String, i32, i32, String)> = sqlx::query_as(
        "SELECT variant, width, height, bucket FROM media.media_derivatives
         WHERE media_id = $1 ORDER BY width DESC",
    )
    .bind(id)
    .fetch_all(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(
        variants,
        vec![
            ("feed_1080".to_owned(), 1080, 810, "quarantine".to_owned()),
            ("widget_512".to_owned(), 512, 384, "quarantine".to_owned()),
            ("thumb_256".to_owned(), 256, 192, "quarantine".to_owned()),
        ]
    );
    // Raw upload (kept for the gates) + the re-encode + 3 derivatives; nothing is clean.
    assert_eq!(env.media_store.count(Bucket::Quarantine), before + 5);
    assert_eq!(env.media_store.count(Bucket::Clean), 0);

    // Running again (job retry) changes nothing.
    env.state
        .media
        .process_one(id, &ProcessSettings::default())
        .await
        .expect_err("unconfigured gates retry");
    assert_eq!(env.media_store.count(Bucket::Quarantine), before + 5);

    let response = call(&env, "GET", &format!("/v1/media/{id}"), &user, None).await;
    let body = json_body(response).await;
    assert_eq!(body["media"]["state"], "scanning");
    assert!(!body.to_string().contains("url"));
}

#[tokio::test]
async fn decoder_timeout_marks_the_media_failed() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let id = uploaded_id(&env, &user, "card_render", real_png(512, 512)).await;
    let settings = ProcessSettings {
        decode_timeout: Duration::ZERO,
        ..ProcessSettings::default()
    };
    // The job itself succeeds (no retry storm); the media is failed for good.
    env.state.media.process_one(id, &settings).await.unwrap();
    let (state, failure, _, processed) = row(&env, id).await;
    assert_eq!(state, "failed");
    assert_eq!(failure.as_deref(), Some("decode_timeout"));
    assert!(!processed);
    // The worker is still usable afterwards.
    let next = uploaded_id(&env, &user, "card_render", real_png(64, 64)).await;
    env.state
        .media
        .process_one(next, &ProcessSettings::default())
        .await
        .expect_err("unconfigured gates retry");
    assert_eq!(row(&env, next).await.0, "scanning");
}

#[tokio::test]
async fn header_only_garbage_fails_to_decode() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let mut bytes = png_header(64, 64);
    bytes.extend_from_slice(b"this is not a zlib stream at all");
    bytes.extend_from_slice(Uuid::now_v7().as_bytes());
    let id = uploaded_id(&env, &user, "card_render", bytes).await;
    env.state
        .media
        .process_one(id, &ProcessSettings::default())
        .await
        .unwrap();
    let (state, failure, _, _) = row(&env, id).await;
    assert_eq!(state, "failed");
    assert_eq!(failure.as_deref(), Some("decode_error"));
    let response = call(&env, "GET", &format!("/v1/media/{id}"), &user, None).await;
    assert_eq!(json_body(response).await["media"]["state"], "failed");
}
