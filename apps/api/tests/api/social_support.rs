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
}
