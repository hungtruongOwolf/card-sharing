//! T-29: minor status from the encrypted DOB, strictest region, OS age signals (US-2, SR-24,
//! SR-25), against real Postgres + Redis (skipped locally when unset, required in CI).

use crate::common::*;
use axum::body::Body;
use http::{Request, StatusCode};
use identity::{AgeStatus, OsAgeSignal, OsAgeSource};
use platform::clock::Clock;
use policy::PolicyContext;
use serde_json::json;
use time::{Date, Duration};
use uuid::Uuid;

/// An active account born on `birth_date`, with `stored` region signals from sign-up.
async fn account_born(env: &TestEnv, birth_date: Date, stored: serde_json::Value) -> Uuid {
    let (account, _) = env.signed_in_user().await;
    let encrypted = field_cipher()
        .encrypt(birth_date.to_string().as_bytes(), account.as_bytes())
        .unwrap();
    sqlx::query(
        "UPDATE identity.accounts SET status = 'active', birth_date_enc = $2, region_signals = $3
         WHERE id = $1",
    )
    .bind(account)
    .bind(encrypted)
    .bind(stored)
    .execute(env.identity.pool())
    .await
    .unwrap();
    account
}

fn years_before(day: Date, years: i32) -> Date {
    day.replace_year(day.year() - years).unwrap()
}

fn ip(country: &str) -> PolicyContext {
    PolicyContext {
        ip_country: Some(country.parse().unwrap()),
        ..PolicyContext::default()
    }
}

#[tokio::test]
async fn turning_18_makes_an_adult_the_next_day_without_any_change() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    // Avoid 29 February: replace_year must exist 18 years back.
    env.clock.set(
        Date::from_calendar_date(2026, time::Month::June, 10)
            .unwrap()
            .midnight()
            .assume_utc()
            + Duration::hours(12),
    );
    let today = env.clock.now().date();
    let birthday_tomorrow = years_before(today, 18) + Duration::days(1);
    let account = account_born(&env, birthday_tomorrow, json!({ "ip": "US" })).await;
    let current = ip("US");

    let before = env.state.age.status(account, &current).await.unwrap();
    assert_eq!(
        before,
        AgeStatus {
            is_minor: true,
            discoverable: false,
            public_links: false,
            ai_effects: false
        }
    );
    // Cached for today: still a minor a few hours later.
    env.clock.advance(Duration::hours(6));
    assert!(
        env.state
            .age
            .status(account, &current)
            .await
            .unwrap()
            .is_minor
    );

    env.clock.advance(Duration::hours(12));
    let after = env.state.age.status(account, &current).await.unwrap();
    assert_eq!(
        after,
        AgeStatus {
            is_minor: false,
            discoverable: true,
            public_links: true,
            ai_effects: true
        }
    );
}

#[tokio::test]
async fn a_minor_os_signal_overrides_an_adult_dob() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let today = env.clock.now().date();
    let account = account_born(&env, years_before(today, 30), json!({ "ip": "US" })).await;
    let current = ip("US");
    assert!(
        !env.state
            .age
            .status(account, &current)
            .await
            .unwrap()
            .is_minor
    );

    env.state
        .age
        .record_os_signal(
            account,
            OsAgeSource::AppleDeclaredAgeRange,
            OsAgeSignal::Minor,
        )
        .await
        .unwrap();
    let status = env.state.age.status(account, &current).await.unwrap();
    assert!(status.is_minor, "the signal evicts the cached adult status");
    assert!(!status.discoverable);

    // An adult signal never loosens a minor DOB.
    let minor = account_born(&env, years_before(today, 15), json!({ "ip": "US" })).await;
    env.state
        .age
        .record_os_signal(minor, OsAgeSource::GooglePlayAgeSignals, OsAgeSignal::Adult)
        .await
        .unwrap();
    assert!(
        env.state
            .age
            .status(minor, &current)
            .await
            .unwrap()
            .is_minor
    );
}

#[tokio::test]
async fn a_later_request_from_a_laxer_region_does_not_lower_discovery_min_age() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let today = env.clock.now().date();
    // 18 years old: adult and discoverable in the US, but KR's age of majority is 19.
    let account = account_born(
        &env,
        years_before(today, 18) - Duration::days(30),
        json!({ "ip": "KR" }),
    )
    .await;
    let from_us = env.state.age.status(account, &ip("US")).await.unwrap();
    assert_eq!(
        from_us,
        AgeStatus {
            is_minor: true,
            discoverable: false,
            public_links: false,
            ai_effects: false
        },
        "stored KR signals still apply"
    );
    let us_account = account_born(
        &env,
        years_before(today, 18) - Duration::days(30),
        json!({ "ip": "US" }),
    )
    .await;
    assert!(
        env.state
            .age
            .status(us_account, &ip("US"))
            .await
            .unwrap()
            .discoverable
    );
    // A current request from KR tightens a US account too.
    assert!(
        !env.state
            .age
            .status(us_account, &ip("KR"))
            .await
            .unwrap()
            .discoverable
    );
}

#[tokio::test]
async fn accounts_without_a_dob_get_every_protection() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, _) = env.signed_in_user().await;
    let status = env
        .state
        .age
        .status(account, &PolicyContext::default())
        .await
        .unwrap();
    assert!(status.is_minor && !status.discoverable);
}

fn signal_request(token: &str) -> Request<Body> {
    post_json(
        "/v1/me/age-signal",
        Some(token),
        &json!({ "source": "google_play_age_signals", "signal": "minor" }),
    )
}

#[tokio::test]
async fn the_age_signal_route_stores_when_enabled_and_is_404_when_disabled() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let today = env.clock.now().date();
    let account = account_born(&env, years_before(today, 30), json!({})).await;
    let session = env.identity.start_session(account, None).await.unwrap();
    let token = session.access_token.expose();

    let response = send(&env.router, signal_request(token)).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let stored: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT os_age_signal, os_age_signal_source FROM identity.accounts WHERE id = $1",
    )
    .bind(account)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(
        stored,
        (
            Some("minor".to_owned()),
            Some("google_play_age_signals".to_owned())
        )
    );

    let invalid = post_json(
        "/v1/me/age-signal",
        Some(token),
        &json!({ "source": "google_play_age_signals", "signal": "minor", "account_id": Uuid::now_v7() }),
    );
    assert_problem(
        send(&env.router, invalid).await,
        StatusCode::BAD_REQUEST,
        "VALIDATION_FAILED",
    )
    .await;

    // Default production setting: off.
    let disabled = api::build(
        api::openapi::router(),
        env.state.clone().with_os_age_signals(false),
        &test_config(),
    );
    let other = account_born(&env, years_before(today, 30), json!({})).await;
    let other_session = env.identity.start_session(other, None).await.unwrap();
    let response = send(
        &disabled,
        signal_request(other_session.access_token.expose()),
    )
    .await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    let nothing: Option<String> =
        sqlx::query_scalar("SELECT os_age_signal FROM identity.accounts WHERE id = $1")
            .bind(other)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(nothing, None);
}
