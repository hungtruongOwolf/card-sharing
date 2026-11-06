//! T-74: per-viewer signed media URLs. The edge serves only `clean` objects for a valid,
//! unexpired signature over the exact key; issuance is budgeted per user.

use crate::common::*;
use crate::media_gate::fake_gates;
use crate::media_process::{real_png, real_png_with, uploaded_id};
use crate::social_support::*;
use api::access::Principal;
use http::StatusCode;
use identity::{AccountStatus, SessionScope};
use image::Rgba;
use media::safety::FAKE_NUDITY_MARKER;
use media::{Bucket, ObjectKey};
use platform::clock::Clock;
use uuid::Uuid;

async fn main_key(env: &TestEnv, id: Uuid) -> ObjectKey {
    let key: String =
        sqlx::query_scalar("SELECT storage_key FROM media.media_objects WHERE id = $1")
            .bind(id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    ObjectKey::parse(&key).unwrap()
}

fn path_of(url: &str) -> String {
    url.strip_prefix("http://localhost").unwrap().to_owned()
}

#[tokio::test]
async fn a_signed_url_serves_the_clean_image_until_it_expires() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let id = uploaded_id(&env, &user, "card_render", real_png(64, 64)).await;
    env.state
        .media
        .process_one(id, &fake_gates())
        .await
        .unwrap();
    let key = main_key(&env, id).await;
    let signed = env.state.sign_media(&key, user.id).unwrap();
    assert!(signed.url.starts_with(TEST_MEDIA_BASE_URL));
    assert_eq!(
        signed.expires_at,
        env.clock.now().replace_nanosecond(0).unwrap() + time::Duration::minutes(10)
    );

    let response = send(&env.router, get_request(&path_of(&signed.url), None)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/webp");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let bytes = body_bytes(response).await;
    assert_eq!(&bytes[..4], b"RIFF");

    env.clock.advance(time::Duration::minutes(10));
    let expired = send(&env.router, get_request(&path_of(&signed.url), None)).await;
    assert_problem(expired, StatusCode::FORBIDDEN, "FORBIDDEN").await;
}

#[tokio::test]
async fn tampered_or_unsigned_urls_are_403() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let a = uploaded_id(&env, &user, "card_render", real_png(32, 32)).await;
    let b = uploaded_id(&env, &user, "card_render", real_png(32, 32)).await;
    for id in [a, b] {
        env.state
            .media
            .process_one(id, &fake_gates())
            .await
            .unwrap();
    }
    let (key_a, key_b) = (main_key(&env, a).await, main_key(&env, b).await);
    let path = path_of(&env.state.sign_media(&key_a, user.id).unwrap().url);

    let swapped = path.replace(key_a.as_str(), key_b.as_str());
    let other_viewer = path.replace(
        &user.id.simple().to_string(),
        &Uuid::now_v7().simple().to_string(),
    );
    let no_query = path.split('?').next().unwrap().to_owned();
    let (head, sig) = path.rsplit_once("sig=").unwrap();
    let flipped = format!(
        "{head}sig={}{}",
        if sig.starts_with('A') { "B" } else { "A" },
        &sig[1..]
    );
    let prefix = format!(
        "/v1/media-files/{}?{}",
        &key_a.as_str()[..16],
        path.split_once('?').unwrap().1
    );
    for bad in [swapped, other_viewer, no_query, flipped, prefix] {
        let response = send(&env.router, get_request(&bad, None)).await;
        assert_problem(response, StatusCode::FORBIDDEN, "FORBIDDEN").await;
    }
}

#[tokio::test]
async fn quarantined_objects_are_never_served_even_with_a_valid_signature() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    // Held by the classifier: its sanitized copy stays in quarantine.
    let id = uploaded_id(
        &env,
        &user,
        "card_render",
        real_png_with(32, 32, Rgba(FAKE_NUDITY_MARKER)),
    )
    .await;
    env.state
        .media
        .process_one(id, &fake_gates())
        .await
        .unwrap();
    let key = main_key(&env, id).await;
    assert!(env.media_store.count(Bucket::Quarantine) > 0);
    let signed = env.state.sign_media(&key, user.id).unwrap();
    let response = send(&env.router, get_request(&path_of(&signed.url), None)).await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
}

#[tokio::test]
async fn url_issuance_is_budgeted_per_user() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let principal = Principal {
        account_id: Uuid::now_v7(),
        session_id: Uuid::now_v7(),
        scope: SessionScope::Full,
        status: AccountStatus::Active,
        account_created_at: env.clock.now() - time::Duration::days(30),
        device_id: None,
    };
    env.state.charge_url_budget(&principal, 600).await.unwrap();
    env.state.charge_url_budget(&principal, 400).await.unwrap();
    let err = env
        .state
        .charge_url_budget(&principal, 1)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "RATE_LIMITED");
    // Another user has their own budget.
    let other = Principal {
        account_id: Uuid::now_v7(),
        ..principal
    };
    env.state.charge_url_budget(&other, 1000).await.unwrap();
}
