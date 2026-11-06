//! Feed (part of #82): delivered cards from current friends, newest first, cursor pages, the
//! same visibility predicate as a card read, per-viewer signed URLs.

use crate::cards_read::deliver_to;
use crate::common::*;
use crate::social_support::*;
use http::StatusCode;
use serde_json::Value;
use uuid::Uuid;

async fn feed(env: &TestEnv, user: &User, query: &str) -> Value {
    let response = call(env, "GET", &format!("/v1/feed{query}"), user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

fn ids(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn newest_first_with_cursor_pages() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let me = adult(&env).await;
    let friend = adult(&env).await;
    make_friends(&env, me.id, friend.id).await;
    let mut sent = Vec::new();
    for _ in 0..3 {
        sent.push(deliver_to(&env, &friend, &[&me]).await.to_string());
        env.clock.advance(time::Duration::seconds(1));
    }
    sent.reverse();

    let first = feed(&env, &me, "?limit=2").await;
    assert_eq!(ids(&first), sent[..2].to_vec());
    let item = &first["items"][0];
    assert_eq!(item["is_mine"], false);
    assert!(
        item["media"]["render_url"]
            .as_str()
            .unwrap()
            .contains(&format!("v={}", me.id.simple()))
    );
    assert!(item["media"].get("original_url").is_none());
    let cursor = first["next_cursor"].as_str().unwrap().to_owned();
    let second = feed(&env, &me, &format!("?limit=2&cursor={cursor}")).await;
    assert_eq!(ids(&second), sent[2..].to_vec());
    assert!(second["next_cursor"].is_null());

    // My own sent cards are not in my feed.
    assert!(ids(&feed(&env, &friend, "").await).is_empty());
}

#[tokio::test]
async fn unfriended_blocked_and_strangers_see_nothing() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let me = adult(&env).await;
    let a = adult(&env).await;
    let b = adult(&env).await;
    make_friends(&env, me.id, a.id).await;
    make_friends(&env, me.id, b.id).await;
    deliver_to(&env, &a, &[&me]).await;
    deliver_to(&env, &b, &[&me]).await;
    assert_eq!(ids(&feed(&env, &me, "").await).len(), 2);

    let response = call(&env, "DELETE", &format!("/v1/friends/{}", a.id), &me, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    block(&env, b.id, me.id).await;
    assert!(ids(&feed(&env, &me, "").await).is_empty());

    let stranger = adult(&env).await;
    assert!(ids(&feed(&env, &stranger, "").await).is_empty());
}

#[tokio::test]
async fn bad_limits_and_tampered_cursors_are_400() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let me = adult(&env).await;
    for query in [
        "?limit=0",
        "?limit=31",
        "?limit=x",
        "?cursor=abc",
        "?sort=asc",
    ] {
        let response = call(&env, "GET", &format!("/v1/feed{query}"), &me, None).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    // A well-formed cursor with a forged tag.
    let forged = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        [Uuid::now_v7().as_bytes().as_slice(), &[0u8; 40]].concat(),
    );
    let response = call(&env, "GET", &format!("/v1/feed?cursor={forged}"), &me, None).await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
}
