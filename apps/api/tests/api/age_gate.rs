//! T-28: region age gate (US-2, AC-2.1–2.3, SR-24, SR-25) against real Postgres + Redis.

use crate::common::*;
use axum::body::Body;
use http::{Request, StatusCode};
use identity::{IssuedSession, Provider, VerifiedIdentity};
use platform::clock::Clock;
use serde_json::{Value, json};
use time::{Date, Month};
use uuid::Uuid;

/// 1 January, `years` ago: the person turned `years` this year (on or before today).
fn born_years_ago(env: &TestEnv, years: i32) -> String {
    let today = env.clock.now().date();
    Date::from_calendar_date(today.year() - years, Month::January, 1)
        .unwrap()
        .to_string()
}

fn submit(token: &str, body: &Value, ip_country: Option<&str>) -> Request<Body> {
    let mut builder = Request::post("/v1/me/age")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"));
    if let Some(country) = ip_country {
        builder = builder.header(TEST_COUNTRY_HEADER, country);
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

async fn status_of(env: &TestEnv, account: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT status FROM identity.accounts WHERE id = $1")
        .bind(account)
        .fetch_optional(env.identity.pool())
        .await
        .unwrap()
}

fn token(session: &IssuedSession) -> &str {
    session.access_token.expose()
}

#[tokio::test]
async fn eu_fifteen_year_old_is_refused_neutrally_and_the_account_is_removed() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, session) = env.signed_in_user().await;
    let body = json!({ "birth_date": born_years_ago(&env, 15) });
    let response = send(&env.router, submit(token(&session), &body, Some("DE"))).await;
    let problem = assert_problem(response, StatusCode::FORBIDDEN, "AGE_GATE_REFUSED").await;
    assert!(
        problem.get("detail").is_none(),
        "neutral: no threshold or region"
    );
    assert!(!problem.to_string().contains("16"));

    assert_eq!(
        status_of(&env, account).await,
        None,
        "pending account deleted"
    );
    let me = send(&env.router, get_request("/v1/me", Some(token(&session)))).await;
    assert_eq!(me.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn same_age_in_a_13_region_is_admitted_with_the_dob_encrypted() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, session) = env.signed_in_user().await;
    let dob = born_years_ago(&env, 15);
    let body = json!({ "birth_date": dob, "declared_country": "US", "store_country": "US" });
    let response = send(&env.router, submit(token(&session), &body, Some("US"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["status"], "pending_profile");
    assert_eq!(body["onboarding"]["step"], "profile");

    // The session sees the new step immediately (cache evicted).
    let me = json_body(send(&env.router, get_request("/v1/me", Some(token(&session)))).await).await;
    assert_eq!(me["onboarding"]["step"], "profile");

    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT birth_date_enc FROM identity.accounts WHERE id = $1")
            .bind(account)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert!(
        !stored.windows(dob.len()).any(|w| w == dob.as_bytes()),
        "DOB never stored in clear"
    );
}

#[tokio::test]
async fn dob_is_locked_after_the_first_submission() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, session) = env.signed_in_user().await;
    let adult = json!({ "birth_date": born_years_ago(&env, 30) });
    assert_eq!(
        send(&env.router, submit(token(&session), &adult, None))
            .await
            .status(),
        StatusCode::OK
    );
    let again = send(&env.router, submit(token(&session), &adult, None)).await;
    assert_problem(again, StatusCode::FORBIDDEN, "AGE_ALREADY_SUBMITTED").await;
}

#[tokio::test]
async fn retry_from_the_same_device_within_24h_is_refused() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let device = Uuid::now_v7();
    let (_, first) = env.signed_in_on(device).await;
    let minor = json!({ "birth_date": born_years_ago(&env, 15) });
    let response = send(&env.router, submit(token(&first), &minor, Some("FR"))).await;
    assert_problem(response, StatusCode::FORBIDDEN, "AGE_GATE_REFUSED").await;

    // A fresh account on the same phone now claims adulthood.
    let (second_account, second) = env.signed_in_on(device).await;
    let adult = json!({ "birth_date": born_years_ago(&env, 30) });
    let response = send(&env.router, submit(token(&second), &adult, Some("FR"))).await;
    assert_problem(response, StatusCode::FORBIDDEN, "AGE_GATE_REFUSED").await;
    assert_eq!(status_of(&env, second_account).await, None);
}

#[tokio::test]
async fn retry_with_the_same_sign_in_identity_within_24h_is_refused() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let identity = VerifiedIdentity {
        provider: Provider::Google,
        subject: format!("google-{}", Uuid::now_v7()),
        email: None,
    };
    let (_, first) = env.signed_in_as(&identity, Uuid::now_v7()).await;
    let minor = json!({ "birth_date": born_years_ago(&env, 15) });
    let response = send(&env.router, submit(token(&first), &minor, Some("DE"))).await;
    assert_problem(response, StatusCode::FORBIDDEN, "AGE_GATE_REFUSED").await;

    let (_, second) = env.signed_in_as(&identity, Uuid::now_v7()).await;
    let adult = json!({ "birth_date": born_years_ago(&env, 30) });
    let response = send(&env.router, submit(token(&second), &adult, None)).await;
    assert_problem(response, StatusCode::FORBIDDEN, "AGE_GATE_REFUSED").await;
}

#[tokio::test]
async fn client_region_hints_cannot_loosen_the_policy() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    // Declared US + EU IP + age 15 -> refused.
    let (_, session) = env.signed_in_user().await;
    let body = json!({ "birth_date": born_years_ago(&env, 15), "declared_country": "US", "store_country": "US" });
    let response = send(&env.router, submit(token(&session), &body, Some("DE"))).await;
    assert_problem(response, StatusCode::FORBIDDEN, "AGE_GATE_REFUSED").await;

    // A store hint can tighten: KR storefront (14) refuses a 13-year-old.
    let (_, session) = env.signed_in_user().await;
    let body = json!({ "birth_date": born_years_ago(&env, 13), "store_country": "KR" });
    let response = send(&env.router, submit(token(&session), &body, None)).await;
    assert_problem(response, StatusCode::FORBIDDEN, "AGE_GATE_REFUSED").await;
}

#[tokio::test]
async fn only_onboarding_sessions_pending_age_may_call() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let body = json!({ "birth_date": born_years_ago(&env, 30) });

    let unauthenticated = post_json("/v1/me/age", None, &body);
    assert_problem(
        send(&env.router, unauthenticated).await,
        StatusCode::UNAUTHORIZED,
        "UNAUTHENTICATED",
    )
    .await;

    // A full session of an active account is not an onboarding session.
    let (account, _) = env.signed_in_user().await;
    sqlx::query("UPDATE identity.accounts SET status = 'active' WHERE id = $1")
        .bind(account)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let full = env.identity.start_session(account, None).await.unwrap();
    let response = send(&env.router, submit(token(&full), &body, None)).await;
    assert_problem(response, StatusCode::FORBIDDEN, "FORBIDDEN").await;
}

#[tokio::test]
async fn invalid_bodies_are_rejected_without_consuming_the_gate() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let tomorrow = (env.clock.now().date() + time::Duration::days(1)).to_string();
    // A fresh user per body: me.age allows 5 submissions per device per day.
    for body in [
        json!({ "birth_date": born_years_ago(&env, 30), "min_age": 0 }),
        json!({ "birth_date": born_years_ago(&env, 30), "account_id": Uuid::now_v7() }),
        json!({ "birth_date": "01/05/2010" }),
        json!({ "birth_date": tomorrow }),
        json!({ "birth_date": "1850-01-01" }),
        json!({ "birth_date": born_years_ago(&env, 30), "declared_country": "USA" }),
    ] {
        let (account, session) = env.signed_in_user().await;
        let response = send(&env.router, submit(token(&session), &body, None)).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
        assert_eq!(
            status_of(&env, account).await.as_deref(),
            Some("pending_age")
        );
    }
}

#[tokio::test]
async fn sixth_submission_from_one_device_in_a_day_is_rate_limited() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, session) = env.signed_in_user().await;
    let bad = json!({ "birth_date": "not-a-date" });
    for _ in 0..5 {
        let response = send(&env.router, submit(token(&session), &bad, None)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let response = send(&env.router, submit(token(&session), &bad, None)).await;
    assert_problem(response, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED").await;
}
