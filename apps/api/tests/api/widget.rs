//! T-91 (US-16, AC-16.3, AC-16.4, SR-14): widget tokens are read-only children of a session;
//! `GET /v1/widget` returns the newest visible card, re-evaluated on every call; any other route
//! refuses a widget token; revoking the parent session kills it.

use crate::cards_read::deliver_to;
use crate::common::*;
use crate::social_support::*;
use http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

/// A widget "user": same account, the widget token as bearer.
async fn widget_of(env: &TestEnv, user: &User) -> User {
    let response = call(env, "POST", "/v1/me/widget-token", user, None).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = json_body(response).await;
    User {
        token: body["token"].as_str().unwrap().to_owned(),
        ..user.clone()
    }
}

async fn widget(env: &TestEnv, user: &User, query: &str) -> Value {
    let response = call(env, "GET", &format!("/v1/widget{query}"), user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

#[tokio::test]
async fn shows_the_newest_visible_card_and_empties_on_unfriend_block_and_delete() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let me = adult(&env).await;
    let a = adult(&env).await;
    let b = adult(&env).await;
    make_friends(&env, me.id, a.id).await;
    make_friends(&env, me.id, b.id).await;
    let w = widget_of(&env, &me).await;
    assert!(widget(&env, &w, "").await["card"].is_null());

    let from_a = deliver_to(&env, &a, &[&me]).await;
    env.clock.advance(time::Duration::seconds(1));
    let from_b = deliver_to(&env, &b, &[&me]).await;
    let shown = widget(&env, &w, "").await;
    let card = &shown["card"];
    assert_eq!(card["id"], json!(from_b));
    assert_eq!(card["sender_name"], json!(b.name));
    assert_eq!(
        card["deep_link"],
        json!(format!("cardsharing://card/{from_b}"))
    );
    assert!(
        card["image_url"]
            .as_str()
            .unwrap()
            .contains(&format!("v={}", me.id.simple()))
    );
    assert!(card.get("caption").is_none());
    // AC-16.3: one friend only; a stranger's id shows nothing; a malformed id is 400.
    assert_eq!(
        widget(&env, &w, &format!("?friend_id={}", a.id)).await["card"]["id"],
        json!(from_a)
    );
    assert!(widget(&env, &w, &format!("?friend_id={}", Uuid::now_v7())).await["card"].is_null());
    let response = call(&env, "GET", "/v1/widget?friend_id=nope", &w, None).await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;

    // AC-16.4: delete, then block, then unfriend: each empties the next call.
    let response = call(&env, "DELETE", &format!("/v1/cards/{from_b}"), &b, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(widget(&env, &w, "").await["card"]["id"], json!(from_a));
    let response = call(&env, "DELETE", &format!("/v1/friends/{}", a.id), &me, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(widget(&env, &w, "").await["card"].is_null());

    // A full session may read it too.
    assert!(widget(&env, &me, "").await["card"].is_null());
}

#[tokio::test]
async fn a_widget_token_is_refused_everywhere_else() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let me = adult(&env).await;
    let w = widget_of(&env, &me).await;
    for (method, path, body) in [
        (
            "POST",
            "/v1/cards",
            Some(json!({ "audience": { "mode": "all" } })),
        ),
        ("GET", "/v1/feed", None),
        ("GET", "/v1/me", None),
        ("GET", "/v1/friends", None),
        ("POST", "/v1/me/widget-token", None),
        ("GET", "/v1/sessions", None),
        ("DELETE", "/v1/sessions/current", None),
        (
            "POST",
            "/v1/feed/purges",
            Some(json!({ "card_ids": [Uuid::now_v7()] })),
        ),
    ] {
        let response = call(&env, method, path, &w, body.as_ref()).await;
        assert_problem(response, StatusCode::FORBIDDEN, "FORBIDDEN").await;
    }
}

#[tokio::test]
async fn revoking_the_parent_or_minting_again_kills_the_old_token() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let me = adult(&env).await;
    let first = widget_of(&env, &me).await;
    let second = widget_of(&env, &me).await;
    let response = call(&env, "GET", "/v1/widget", &first, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    widget(&env, &second, "").await;

    // Logging out the parent session revokes its widget token.
    let response = call(&env, "DELETE", "/v1/sessions/current", &me, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = call(&env, "GET", "/v1/widget", &second, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Suspension too.
    let other = adult(&env).await;
    let w = widget_of(&env, &other).await;
    env.identity
        .suspend_account(other.id, "test")
        .await
        .unwrap();
    let response = call(&env, "GET", "/v1/widget", &w, None).await;
    assert!(
        matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ),
        "{}",
        response.status()
    );
}
