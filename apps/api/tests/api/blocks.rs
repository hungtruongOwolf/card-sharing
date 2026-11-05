//! T-47: block and unblock silently (US-26 AC-26.2, SR-22) against real Postgres + Redis.

use crate::common::*;
use crate::social_support::*;
use axum::body::Body;
use http::{Request, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

fn block_request(user: &User, target: &str, request_id: Uuid) -> Request<Body> {
    Request::post("/v1/blocks")
        .header("authorization", format!("Bearer {}", user.token))
        .header("content-type", "application/json")
        .header("x-request-id", request_id.to_string())
        .body(Body::from(json!({ "user_id": target }).to_string()))
        .unwrap()
}

async fn block_via_api(env: &TestEnv, user: &User, target: Uuid) -> StatusCode {
    send(
        &env.router,
        block_request(user, &target.to_string(), Uuid::now_v7()),
    )
    .await
    .status()
}

async fn items(env: &TestEnv, user: &User, path: &str) -> Vec<Value> {
    let response = call(env, "GET", path, user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["items"]
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn blocking_a_friend_unfriends_and_their_re_request_is_silently_dropped() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    make_friends(&env, alice.id, bob.id).await;

    assert_eq!(
        block_via_api(&env, &alice, bob.id).await,
        StatusCode::NO_CONTENT
    );
    assert!(items(&env, &alice, "/v1/friends").await.is_empty());
    assert!(items(&env, &bob, "/v1/friends").await.is_empty());
    let blocks = items(&env, &alice, "/v1/blocks").await;
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["user"]["id"], bob.id.to_string());

    // AC-26.2: Bob's new request gets the standard 201, but Alice never sees it.
    let token = invite_token(&env, &alice).await;
    let response = request_via_invite(&env, &bob, &token).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = json_body(response).await["request"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(request_status(&env, &id).await, "dropped");
    assert!(
        items(&env, &alice, "/v1/friend-requests?direction=incoming")
            .await
            .is_empty()
    );
    // Bob's side looks exactly like a pending request.
    let outgoing = items(&env, &bob, "/v1/friend-requests?direction=outgoing").await;
    assert_eq!(outgoing.len(), 1);
    // Nothing was published to Bob: no request event, and the block event is internal.
    let topics: Vec<String> = sqlx::query_scalar(
        "SELECT topic FROM platform.outbox_events WHERE payload::text LIKE '%' || $1 || '%' ORDER BY id",
    )
    .bind(bob.id.to_string())
    .fetch_all(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(topics, ["social.friendship_removed", "social.user_blocked"]);
}

#[tokio::test]
async fn blocking_drops_their_pending_requests_and_cancels_mine() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let to_alice = json_body(
        request_via_invite(&env, &bob, &invite_token(&env, &alice).await).await,
    )
    .await["request"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let to_bob = json_body(request_via_invite(&env, &alice, &invite_token(&env, &bob).await).await)
        .await["request"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    assert_eq!(
        block_via_api(&env, &alice, bob.id).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(request_status(&env, &to_alice).await, "dropped");
    assert_eq!(request_status(&env, &to_bob).await, "cancelled");
    assert!(
        items(&env, &alice, "/v1/friend-requests?direction=incoming")
            .await
            .is_empty()
    );
    assert!(
        items(&env, &bob, "/v1/friend-requests?direction=incoming")
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn blocking_an_unrelated_account_is_a_404_like_a_missing_id() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let stranger = adult(&env).await;
    let request_id = Uuid::now_v7();
    let denied = send(
        &env.router,
        block_request(&alice, &stranger.id.to_string(), request_id),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    let missing = send(
        &env.router,
        block_request(&alice, &Uuid::now_v7().to_string(), request_id),
    )
    .await;
    assert_eq!(body_bytes(denied).await, body_bytes(missing).await);
    for target in [alice.id.to_string(), "not-an-id".to_owned()] {
        let response = send(&env.router, block_request(&alice, &target, Uuid::now_v7())).await;
        assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    }
    assert!(items(&env, &alice, "/v1/blocks").await.is_empty());
}

#[tokio::test]
async fn unblock_lifts_the_block_but_not_the_friendship() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    make_friends(&env, alice.id, bob.id).await;
    assert_eq!(
        block_via_api(&env, &alice, bob.id).await,
        StatusCode::NO_CONTENT
    );
    // Blocking again is harmless.
    assert_eq!(
        block_via_api(&env, &alice, bob.id).await,
        StatusCode::NO_CONTENT
    );

    let response = call(
        &env,
        "DELETE",
        &format!("/v1/blocks/{}", bob.id),
        &alice,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(items(&env, &alice, "/v1/blocks").await.is_empty());
    assert!(items(&env, &alice, "/v1/friends").await.is_empty());
    // Only the blocker can lift it; anything else is 404.
    let again = call(
        &env,
        "DELETE",
        &format!("/v1/blocks/{}", bob.id),
        &alice,
        None,
    )
    .await;
    assert_problem(again, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    assert_eq!(
        block_via_api(&env, &alice, bob.id).await,
        StatusCode::NO_CONTENT
    );
    let by_bob = call(
        &env,
        "DELETE",
        &format!("/v1/blocks/{}", alice.id),
        &bob,
        None,
    )
    .await;
    assert_problem(by_bob, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    // After unblocking, a new request reaches Alice again.
    call(
        &env,
        "DELETE",
        &format!("/v1/blocks/{}", bob.id),
        &alice,
        None,
    )
    .await;
    let token = invite_token(&env, &alice).await;
    let id = json_body(request_via_invite(&env, &bob, &token).await).await["request"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(request_status(&env, &id).await, "pending");
}

#[tokio::test]
async fn the_blocked_account_cannot_find_or_preview_the_blocker() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    make_friends(&env, alice.id, bob.id).await;
    let token = invite_token(&env, &alice).await;
    assert_eq!(
        block_via_api(&env, &alice, bob.id).await,
        StatusCode::NO_CONTENT
    );

    let search = Request::post("/v1/users/search")
        .header("authorization", format!("Bearer {}", bob.token))
        .header("content-type", "application/json")
        .body(Body::from(json!({ "handle": alice.handle }).to_string()))
        .unwrap();
    assert_eq!(
        json_body(send(&env.router, search).await).await,
        json!({ "result": null })
    );
    let preview = call(&env, "GET", &format!("/v1/invites/{token}"), &bob, None).await;
    assert_problem(preview, StatusCode::NOT_FOUND, "NOT_FOUND").await;
}
