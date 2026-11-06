//! T-84: private reactions. Recipients who may see a card react (one each, replaceable,
//! removable); only the owner lists them; other recipients never see them; anyone else gets the
//! missing-card 404.

use crate::cards_read::{deliver_to, read};
use crate::common::*;
use crate::media::without_request_id;
use crate::social_support::*;
use http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

async fn react(
    env: &TestEnv,
    user: &User,
    card: Uuid,
    kind: &str,
) -> http::Response<axum::body::Body> {
    call(
        env,
        "PUT",
        &format!("/v1/cards/{card}/reaction"),
        user,
        Some(&json!({ "kind": kind })),
    )
    .await
}

async fn reactions(env: &TestEnv, user: &User, card: Uuid) -> http::Response<axum::body::Body> {
    call(
        env,
        "GET",
        &format!("/v1/cards/{card}/reactions"),
        user,
        None,
    )
    .await
}

async fn same_as_missing(
    env: &TestEnv,
    response: http::Response<axum::body::Body>,
    again: http::Response<axum::body::Body>,
) {
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        without_request_id(body_bytes(response).await),
        without_request_id(body_bytes(again).await)
    );
    let _ = env;
}

#[tokio::test]
async fn one_reaction_per_recipient_replaced_and_removed() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let b = adult(&env).await;
    let c = adult(&env).await;
    make_friends(&env, owner.id, b.id).await;
    make_friends(&env, owner.id, c.id).await;
    let card = deliver_to(&env, &owner, &[&b, &c]).await;

    assert_eq!(
        react(&env, &b, card, "react.heart").await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        react(&env, &b, card, "react.squish").await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        react(&env, &c, card, "react.fire").await.status(),
        StatusCode::NO_CONTENT
    );

    let list: Value = json_body(reactions(&env, &owner, card).await).await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "one per recipient (AC-17.2)");
    let by_b = items
        .iter()
        .find(|i| i["user"]["id"] == b.id.to_string())
        .unwrap();
    assert_eq!(by_b["kind"], "react.squish");
    assert_eq!(by_b["user"]["display_name"], b.name.as_str());

    // Each recipient sees only their own reaction (AC-17.1).
    let view_b = json_body(read(&env, &b, card).await).await;
    assert_eq!(view_b["card"]["my_reaction"], "react.squish");
    assert!(!view_b.to_string().contains("react.fire"));
    let view_c = json_body(read(&env, &c, card).await).await;
    assert_eq!(view_c["card"]["my_reaction"], "react.fire");

    // Removing.
    let removed = call(
        &env,
        "DELETE",
        &format!("/v1/cards/{card}/reaction"),
        &b,
        None,
    )
    .await;
    assert_eq!(removed.status(), StatusCode::NO_CONTENT);
    let list: Value = json_body(reactions(&env, &owner, card).await).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert!(json_body(read(&env, &b, card).await).await["card"]["my_reaction"].is_null());
    let event_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.outbox_events WHERE topic = 'cards.reacted' AND payload->>'card_id' = $1",
    )
    .bind(card.to_string())
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(event_count, 3);
}

#[tokio::test]
async fn non_recipients_and_recipients_listing_get_the_missing_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let b = adult(&env).await;
    let stranger = adult(&env).await;
    make_friends(&env, owner.id, b.id).await;
    let card = deliver_to(&env, &owner, &[&b]).await;
    let missing = Uuid::now_v7();

    // A stranger reacting, the owner reacting to their own card.
    for user in [&stranger, &owner] {
        same_as_missing(
            &env,
            react(&env, user, card, "react.heart").await,
            react(&env, user, missing, "react.heart").await,
        )
        .await;
    }
    // A recipient listing all reactions.
    same_as_missing(
        &env,
        reactions(&env, &b, card).await,
        reactions(&env, &b, missing).await,
    )
    .await;
    // After unfriending, the former recipient can no longer react.
    let response = call(
        &env,
        "DELETE",
        &format!("/v1/friends/{}", b.id),
        &owner,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    same_as_missing(
        &env,
        react(&env, &b, card, "react.heart").await,
        react(&env, &b, missing, "react.heart").await,
    )
    .await;
}

#[tokio::test]
async fn unknown_or_switched_off_reactions_are_400() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let b = adult(&env).await;
    make_friends(&env, owner.id, b.id).await;
    let card = deliver_to(&env, &owner, &[&b]).await;
    for kind in ["react.unknown", "heart", ""] {
        let response = react(&env, &b, card, kind).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    let extra = call(
        &env,
        "PUT",
        &format!("/v1/cards/{card}/reaction"),
        &b,
        Some(&json!({ "kind": "react.heart", "reactor_id": owner.id })),
    )
    .await;
    assert_problem(extra, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
}
