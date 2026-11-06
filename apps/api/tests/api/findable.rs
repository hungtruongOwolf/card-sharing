//! T-45: "Findable by handle" (US-5a, SR-19) against real Postgres + Redis.

use crate::common::*;
use crate::social_support::*;
use axum::body::Body;
use http::{Request, StatusCode};
use platform::clock::Clock;
use serde_json::{Value, json};
use time::{Date, Duration, Month};
use uuid::Uuid;

fn patch_settings(user: &User, findable: bool) -> Request<Body> {
    Request::patch("/v1/me/settings")
        .header("authorization", format!("Bearer {}", user.token))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "findable_by_handle": findable }).to_string(),
        ))
        .unwrap()
}

fn search_request(user: &User, handle: &str, request_id: Uuid) -> Request<Body> {
    Request::post("/v1/users/search")
        .header("authorization", format!("Bearer {}", user.token))
        .header("content-type", "application/json")
        .header("x-request-id", request_id.to_string())
        .body(Body::from(json!({ "handle": handle }).to_string()))
        .unwrap()
}

async fn search(env: &TestEnv, user: &User, handle: &str) -> Value {
    json_body(send(&env.router, search_request(user, handle, Uuid::now_v7())).await).await
}

#[tokio::test]
async fn turning_it_off_gives_the_standard_miss() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let searcher = adult(&env).await;
    let target = adult(&env).await;
    assert!(search(&env, &searcher, &target.handle).await["result"].is_object());

    let response = send(&env.router, patch_settings(&target, false)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!({ "findable_by_handle": false, "can_be_findable": true })
    );
    let request_id = Uuid::now_v7();
    let hidden = send(
        &env.router,
        search_request(&searcher, &target.handle, request_id),
    )
    .await;
    let missing = send(
        &env.router,
        search_request(&searcher, &fresh_handle(), request_id),
    )
    .await;
    assert_eq!(body_bytes(hidden).await, body_bytes(missing).await);

    // Back on: found again; GET /v1/me reflects it.
    let response = send(&env.router, patch_settings(&target, true)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(search(&env, &searcher, &target.handle).await["result"].is_object());
    let me = json_body(call(&env, "GET", "/v1/me", &target, None).await).await;
    assert_eq!(me["settings"]["findable_by_handle"], true);
}

#[tokio::test]
async fn a_minor_cannot_turn_it_on() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let teen = minor(&env).await;
    assert_problem(
        send(&env.router, patch_settings(&teen, true)).await,
        StatusCode::FORBIDDEN,
        "POLICY_DISALLOWED",
    )
    .await;
    let response = send(&env.router, patch_settings(&teen, false)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let me = json_body(call(&env, "GET", "/v1/me", &teen, None).await).await;
    assert_eq!(
        me["settings"],
        json!({ "findable_by_handle": false, "can_be_findable": false })
    );
}

#[tokio::test]
async fn a_request_from_an_old_ref_is_dropped_once_the_target_hides() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let searcher = adult(&env).await;
    let target = adult(&env).await;
    let search_ref = search(&env, &searcher, &target.handle).await["result"]["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    send(&env.router, patch_settings(&target, false)).await;
    let request = Request::post("/v1/friend-requests")
        .header("authorization", format!("Bearer {}", searcher.token))
        .header("content-type", "application/json")
        .header("idempotency-key", Uuid::now_v7().to_string())
        .body(Body::from(json!({ "search_ref": search_ref }).to_string()))
        .unwrap();
    let response = send(&env.router, request).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = json_body(response).await["request"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(request_status(&env, &id).await, "dropped");
}

#[tokio::test]
async fn unknown_fields_are_rejected() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let response = call(
        &env,
        "PATCH",
        "/v1/me/settings",
        &user,
        Some(&json!({ "findable_by_handle": true, "account_id": Uuid::now_v7() })),
    )
    .await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
}

/// The stored flag, read straight from the table.
async fn stored_flag(env: &TestEnv, account: Uuid) -> Option<bool> {
    sqlx::query_scalar("SELECT findable_by_handle FROM identity.profiles WHERE account_id = $1")
        .bind(account)
        .fetch_optional(env.identity.pool())
        .await
        .unwrap()
}

/// Passes the real age gate (US signals) as someone who turned `years` on 1 January.
async fn admitted_at(env: &TestEnv, years: i32) -> Uuid {
    let (account, session) = env.signed_in_user().await;
    let today = env.clock.now().date();
    let dob = Date::from_calendar_date(today.year() - years, Month::January, 1).unwrap();
    let request = Request::post("/v1/me/age")
        .header("content-type", "application/json")
        .header(
            "authorization",
            format!("Bearer {}", session.access_token.expose()),
        )
        .header(TEST_COUNTRY_HEADER, "US")
        .body(Body::from(
            json!({ "birth_date": dob.to_string(), "declared_country": "US", "store_country": "US" })
                .to_string(),
        ))
        .unwrap();
    assert_eq!(send(&env.router, request).await.status(), StatusCode::OK);
    account
}

#[tokio::test]
async fn the_age_gate_starts_adults_findable_and_minors_hidden() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let grown_up = admitted_at(&env, 30).await;
    let teen = admitted_at(&env, 15).await;
    assert_eq!(stored_flag(&env, grown_up).await, Some(true));
    assert_eq!(stored_flag(&env, teen).await, Some(false));
}

#[tokio::test]
async fn turning_18_stays_hidden_until_the_user_opts_in() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let teen = user_aged(&env, 17).await;
    assert_eq!(stored_flag(&env, teen.id).await, Some(false));

    // A year later the account is 18 (discovery age) but still hidden.
    env.clock.advance(Duration::days(366));
    let grown_up = signed_in_again(&env, &teen).await;
    let searcher = signed_in_again(&env, &adult(&env).await).await;
    let request_id = Uuid::now_v7();
    let hidden = send(
        &env.router,
        search_request(&searcher, &grown_up.handle, request_id),
    )
    .await;
    let missing = send(
        &env.router,
        search_request(&searcher, &fresh_handle(), request_id),
    )
    .await;
    assert_eq!(body_bytes(hidden).await, body_bytes(missing).await);
    let me = json_body(call(&env, "GET", "/v1/me", &grown_up, None).await).await;
    assert_eq!(
        me["settings"],
        json!({ "findable_by_handle": false, "can_be_findable": true })
    );

    // Opting in makes the account findable.
    let response = send(&env.router, patch_settings(&grown_up, true)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!({ "findable_by_handle": true, "can_be_findable": true })
    );
    assert!(search(&env, &searcher, &grown_up.handle).await["result"].is_object());
}
