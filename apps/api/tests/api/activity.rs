//! T-88 (US-31 AC-31.3): every alert-worthy event lands in the recipient's activity tab, push or
//! not; only the caller's own items; items the caller may no longer see drop out; read marks
//! never move backwards.

use crate::cards_read::deliver_to;
use crate::common::*;
use crate::media::without_request_id;
use crate::social_support::*;
use http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

/// Records every outbox event about `card` into activity, as the job would.
async fn record_events(env: &TestEnv, card: Uuid) {
    let events: Vec<(i64, String, Value)> = sqlx::query_as(
        "SELECT id, topic, payload FROM platform.outbox_events
         WHERE payload->>'card_id' = $1 ORDER BY id",
    )
    .bind(card.to_string())
    .fetch_all(env.identity.pool())
    .await
    .unwrap();
    assert!(!events.is_empty());
    for (id, topic, payload) in events {
        env.state
            .activity
            .record(id, &topic, &payload)
            .await
            .unwrap();
    }
}

async fn activity(env: &TestEnv, user: &User, query: &str) -> Value {
    let response = call(env, "GET", &format!("/v1/activity{query}"), user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

fn kinds(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["type"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn new_cards_and_reactions_appear_for_their_recipient_only() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let friend = adult(&env).await;
    let stranger = adult(&env).await;
    make_friends(&env, owner.id, friend.id).await;
    let card = deliver_to(&env, &owner, &[&friend]).await;
    let response = call(
        &env,
        "PUT",
        &format!("/v1/cards/{card}/reaction"),
        &friend,
        Some(&json!({ "kind": "react.heart" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    record_events(&env, card).await;
    // Recording twice changes nothing (a re-run job).
    record_events(&env, card).await;

    let mine = activity(&env, &friend, "").await;
    assert_eq!(kinds(&mine), vec!["card.new"]);
    let item = &mine["items"][0];
    assert_eq!(item["subject_id"], json!(card));
    assert_eq!(item["actor_id"], json!(owner.id));
    assert_eq!(item["unread"], true);
    assert_eq!(mine["unread_count"], 1);
    // No content in the item: ids, type, time and the read flag only.
    let keys: Vec<&String> = item.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        vec![
            "actor_id",
            "created_at",
            "id",
            "subject_id",
            "type",
            "unread"
        ]
    );

    let theirs = activity(&env, &owner, "").await;
    assert_eq!(kinds(&theirs), vec!["reaction.new"]);
    assert_eq!(theirs["items"][0]["actor_id"], json!(friend.id));
    assert!(kinds(&activity(&env, &stranger, "").await).is_empty());

    // Marking read: only my own items; never backwards.
    let id = mine["items"][0]["id"].as_str().unwrap().to_owned();
    let response = call(
        &env,
        "POST",
        "/v1/activity/read",
        &owner,
        Some(&json!({ "up_to": id })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let denied = without_request_id(body_bytes(response).await);
    let missing = call(
        &env,
        "POST",
        "/v1/activity/read",
        &owner,
        Some(&json!({ "up_to": Uuid::now_v7() })),
    )
    .await;
    assert_eq!(without_request_id(body_bytes(missing).await), denied);
    let response = call(
        &env,
        "POST",
        "/v1/activity/read",
        &friend,
        Some(&json!({ "up_to": id })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let after = activity(&env, &friend, "").await;
    assert_eq!(after["unread_count"], 0);
    assert_eq!(after["items"][0]["unread"], false);
    assert_eq!(activity(&env, &owner, "").await["unread_count"], 1);
}

#[tokio::test]
async fn unfriend_block_and_delete_remove_items() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let me = adult(&env).await;
    let a = adult(&env).await;
    let b = adult(&env).await;
    make_friends(&env, me.id, a.id).await;
    make_friends(&env, me.id, b.id).await;
    let from_a = deliver_to(&env, &a, &[&me]).await;
    let from_b = deliver_to(&env, &b, &[&me]).await;
    record_events(&env, from_a).await;
    record_events(&env, from_b).await;
    // A friendship_created event naming a as my new friend.
    let event: Value =
        json!({ "request_id": Uuid::now_v7(), "sender_id": me.id, "recipient_id": a.id });
    env.state
        .activity
        .record(i64::MAX - 7, "social.friendship_created", &event)
        .await
        .unwrap();
    assert_eq!(
        kinds(&activity(&env, &me, "").await),
        vec!["friend.accepted", "card.new", "card.new"]
    );

    let response = call(&env, "DELETE", &format!("/v1/cards/{from_b}"), &b, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        kinds(&activity(&env, &me, "").await),
        vec!["friend.accepted", "card.new"]
    );
    block(&env, a.id, me.id).await;
    let page = activity(&env, &me, "").await;
    assert!(kinds(&page).is_empty());
    assert_eq!(page["unread_count"], 0);
}

#[tokio::test]
async fn pages_use_a_signed_cursor_and_bad_queries_are_400() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let me = adult(&env).await;
    let friend = adult(&env).await;
    make_friends(&env, me.id, friend.id).await;
    let mut cards = Vec::new();
    for _ in 0..3 {
        let card = deliver_to(&env, &friend, &[&me]).await;
        record_events(&env, card).await;
        cards.push(card);
    }
    let first = activity(&env, &me, "?limit=2").await;
    assert_eq!(first["items"].as_array().unwrap().len(), 2);
    assert_eq!(first["items"][0]["subject_id"], json!(cards[2]));
    let cursor = first["next_cursor"].as_str().unwrap().to_owned();
    let second = activity(&env, &me, &format!("?limit=2&cursor={cursor}")).await;
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert_eq!(second["items"][0]["subject_id"], json!(cards[0]));
    assert!(second["next_cursor"].is_null());

    for query in ["?limit=0", "?limit=51", "?cursor=abc", "?sort=asc"] {
        let response = call(&env, "GET", &format!("/v1/activity{query}"), &me, None).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    let forged = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        [Uuid::now_v7().as_bytes().as_slice(), &[0u8; 32]].concat(),
    );
    let response = call(
        &env,
        "GET",
        &format!("/v1/activity?cursor={forged}"),
        &me,
        None,
    )
    .await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    let response = call(
        &env,
        "POST",
        "/v1/activity/read",
        &me,
        Some(&json!({ "up_to": Uuid::now_v7(), "all": true })),
    )
    .await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
}
