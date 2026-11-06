//! T-83 (AC-6.1, AC-14.1, SR-9): `POST /v1/feed/purges` lists the cached cards the app must drop.
//! Unfriend, block (either way) and delete take effect on the very next check, well within the
//! 60 s budget; unknown ids look exactly like purged ones; visible cards are never listed.

use crate::cards_read::deliver_to;
use crate::common::*;
use crate::social_support::*;
use http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

async fn purges(env: &TestEnv, user: &User, ids: &[Uuid]) -> Vec<Uuid> {
    let response = call(
        env,
        "POST",
        "/v1/feed/purges",
        user,
        Some(&json!({ "card_ids": ids })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = json_body(response).await;
    body["purge"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| Uuid::parse_str(v.as_str().unwrap()).unwrap())
        .collect()
}

#[tokio::test]
async fn unfriend_block_and_delete_purge_on_the_next_check() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let me = adult(&env).await;
    let a = adult(&env).await;
    let b = adult(&env).await;
    let c = adult(&env).await;
    let d = adult(&env).await;
    for friend in [&a, &b, &c, &d] {
        make_friends(&env, me.id, friend.id).await;
    }
    let from_a = deliver_to(&env, &a, &[&me]).await;
    let from_b = deliver_to(&env, &b, &[&me]).await;
    let from_c = deliver_to(&env, &c, &[&me]).await;
    let from_d = deliver_to(&env, &d, &[&me]).await;
    let mine = deliver_to(&env, &me, &[&a]).await;
    let cached = [from_a, from_b, from_c, from_d, mine];
    assert!(purges(&env, &me, &cached).await.is_empty());

    // I unfriend a; b blocks me; c deletes their card. d stays.
    let response = call(&env, "DELETE", &format!("/v1/friends/{}", a.id), &me, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = call(
        &env,
        "POST",
        "/v1/blocks",
        &b,
        Some(&json!({ "user_id": me.id })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = call(&env, "DELETE", &format!("/v1/cards/{from_c}"), &c, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    assert_eq!(
        purges(&env, &me, &cached).await,
        vec![from_a, from_b, from_c]
    );
    // Both sides of an unfriend purge: a no longer keeps my card.
    assert_eq!(purges(&env, &a, &[from_a, mine]).await, vec![mine]);
    // The owner keeps their own (undeleted) card; c's deleted card is purged for c too.
    assert_eq!(purges(&env, &c, &[from_c]).await, vec![from_c]);
}

#[tokio::test]
async fn unknown_and_foreign_ids_look_purged_and_duplicates_collapse() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let friend = adult(&env).await;
    let stranger = adult(&env).await;
    make_friends(&env, owner.id, friend.id).await;
    let card = deliver_to(&env, &owner, &[&friend]).await;
    let unknown = Uuid::now_v7();

    assert_eq!(
        purges(&env, &stranger, &[card, unknown, card]).await,
        vec![card, unknown]
    );
    assert!(purges(&env, &friend, &[card, card]).await.is_empty());
}

#[tokio::test]
async fn bad_bodies_are_400() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let me = adult(&env).await;
    let too_many: Vec<Uuid> = (0..101).map(|_| Uuid::now_v7()).collect();
    for body in [
        json!({ "card_ids": [] }),
        json!({ "card_ids": too_many }),
        json!({ "card_ids": ["not-a-uuid"] }),
        json!({ "card_ids": [Uuid::now_v7()], "extra": 1 }),
        json!({}),
    ] {
        let response = call(&env, "POST", "/v1/feed/purges", &me, Some(&body)).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
}
