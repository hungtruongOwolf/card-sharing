//! Fixtures for the social tests (invites, friend requests, search, blocks): active accounts with
//! a date of birth, a display name and a handle, written straight to the identity tables.

use crate::common::*;
use axum::body::Body;
use http::{Request, Response};
use platform::clock::Clock;
use serde_json::Value;
use time::{Date, Duration};
use uuid::Uuid;

/// An active account with a full session.
#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub token: String,
    pub handle: String,
    pub name: String,
}

/// A valid handle no other test uses.
pub fn fresh_handle() -> String {
    format!("u{}", &Uuid::now_v7().simple().to_string()[18..])
}

/// An active `age`-year-old (region signals: US) with a profile and a full session.
pub async fn user_aged(env: &TestEnv, age: i32) -> User {
    let (id, _) = env.signed_in_user().await;
    let today = env.clock.now().date();
    // Birthday yesterday: exactly `age` today (29 February falls back to the day before).
    let anniversary: Date = today
        .replace_year(today.year() - age)
        .or_else(|_| (today - Duration::days(1)).replace_year(today.year() - age))
        .unwrap();
    let birth = anniversary - Duration::days(1);
    let encrypted = field_cipher()
        .encrypt(birth.to_string().as_bytes(), id.as_bytes())
        .unwrap();
    sqlx::query(
        "UPDATE identity.accounts SET status = 'active', birth_date_enc = $2,
         region_signals = '{\"ip\": \"US\"}'::jsonb WHERE id = $1",
    )
    .bind(id)
    .bind(encrypted)
    .execute(env.identity.pool())
    .await
    .unwrap();
    let handle = fresh_handle();
    let name = format!("Friend {}", &handle[1..5]);
    let now = env.clock.now();
    sqlx::query(
        "INSERT INTO identity.profiles (account_id, display_name, handle, handle_claimed_at, handle_changed_at)
         VALUES ($1, $2, $3, $4, $4)",
    )
    .bind(id)
    .bind(&name)
    .bind(&handle)
    .bind(now)
    .execute(env.identity.pool())
    .await
    .unwrap();
    let session = env.identity.start_session(id, None).await.unwrap();
    User {
        id,
        token: session.access_token.expose().to_owned(),
        handle,
        name,
    }
}

pub async fn adult(env: &TestEnv) -> User {
    user_aged(env, 30).await
}

pub async fn minor(env: &TestEnv) -> User {
    user_aged(env, 15).await
}

/// A request with a JSON body (or none) and an optional bearer token.
pub fn json_request(
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&Value>,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

pub async fn call(
    env: &TestEnv,
    method: &str,
    path: &str,
    user: &User,
    body: Option<&Value>,
) -> Response<Body> {
    send(
        &env.router,
        json_request(method, path, Some(&user.token), body),
    )
    .await
}

/// The token part of an invite URL.
pub fn token_of(url: &str) -> String {
    url.rsplit('/').next().unwrap().to_owned()
}

/// The same account with a new session at the current (manual) clock time, after the clock moved
/// past the access-token lifetime.
pub async fn signed_in_again(env: &TestEnv, user: &User) -> User {
    let session = env.identity.start_session(user.id, None).await.unwrap();
    User {
        token: session.access_token.expose().to_owned(),
        ..user.clone()
    }
/// The caller's invite token (creates the invite).
pub async fn invite_token(env: &TestEnv, user: &User) -> String {
    let response = call(env, "GET", "/v1/me/invite", user, None).await;
    assert_eq!(response.status(), http::StatusCode::OK);
    token_of(json_body(response).await["url"].as_str().unwrap())
}

/// `POST /v1/friend-requests` from an invite token, with a fresh Idempotency-Key.
pub async fn request_via_invite(env: &TestEnv, from: &User, token: &str) -> Response<Body> {
    let request = Request::post("/v1/friend-requests")
        .header("authorization", format!("Bearer {}", from.token))
        .header("content-type", "application/json")
        .header("idempotency-key", Uuid::now_v7().to_string())
        .body(Body::from(
            serde_json::json!({ "invite_token": token }).to_string(),
        ))
        .unwrap();
    send(&env.router, request).await
}

/// Writes a friendship row directly (canonical order).
pub async fn make_friends(env: &TestEnv, a: Uuid, b: Uuid) {
    let (low, high) = if a < b { (a, b) } else { (b, a) };
    sqlx::query("INSERT INTO social.friendships (user_a, user_b, created_at) VALUES ($1, $2, $3)")
        .bind(low)
        .bind(high)
        .bind(env.clock.now())
        .execute(env.identity.pool())
        .await
        .unwrap();
}

/// Writes a block row directly (`blocker` blocks `blocked`).
pub async fn block(env: &TestEnv, blocker: Uuid, blocked: Uuid) {
    sqlx::query(
        "INSERT INTO social.blocks (blocker_id, blocked_id, created_at) VALUES ($1, $2, $3)",
    )
    .bind(blocker)
    .bind(blocked)
    .bind(env.clock.now())
    .execute(env.identity.pool())
    .await
    .unwrap();
}

/// Status of the request with this id.
pub async fn request_status(env: &TestEnv, id: &str) -> String {
    sqlx::query_scalar("SELECT status FROM social.friend_requests WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap()
}
