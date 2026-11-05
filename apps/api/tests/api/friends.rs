//! T-46: friends list and silent unfriend (US-6 AC-6.2, SR-4) against real Postgres + Redis.

use crate::common::*;
use crate::social_support::*;
use axum::body::Body;
use http::{Request, StatusCode};
use serde_json::Value;
use uuid::Uuid;

async fn friend_ids(env: &TestEnv, user: &User) -> Vec<String> {
    let response = call(env, "GET", "/v1/friends", user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect()
}

/// Outbox events that mention `id`, by topic.
async fn events_about(env: &TestEnv, id: Uuid) -> Vec<(String, Value)> {
    sqlx::query_as(
        "SELECT topic, payload FROM platform.outbox_events
         WHERE payload::text LIKE '%' || $1 || '%' ORDER BY id",
    )
    .bind(id.to_string())
    .fetch_all(env.identity.pool())
    .await
    .unwrap()
}

fn delete_request(user: &User, path: &str, request_id: Uuid) -> Request<Body> {
    Request::delete(path)
        .header("authorization", format!("Bearer {}", user.token))
        .header("x-request-id", request_id.to_string())
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn friends_are_listed_with_their_public_profile() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let carol = adult(&env).await;
    make_friends(&env, alice.id, bob.id).await;
    make_friends(&env, bob.id, carol.id).await;

    let response = call(&env, "GET", "/v1/friends", &alice, None).await;
    let items = json_body(response).await["items"].clone();
    assert_eq!(
        items,
        serde_json::json!([{ "id": bob.id, "display_name": bob.name, "handle": bob.handle }])
    );
    assert_eq!(friend_ids(&env, &bob).await.len(), 2);
}

#[tokio::test]
async fn unfriending_is_mutual_and_silent() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    make_friends(&env, alice.id, bob.id).await;

    let response = call(
        &env,
        "DELETE",
        &format!("/v1/friends/{}", bob.id),
        &alice,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(friend_ids(&env, &alice).await.is_empty());
    assert!(friend_ids(&env, &bob).await.is_empty());

    // Nothing addressed to Bob: the only event is the internal friendship_removed.
    let events = events_about(&env, bob.id).await;
    let topics: Vec<&str> = events.iter().map(|(topic, _)| topic.as_str()).collect();
    assert_eq!(topics, [social::FRIENDSHIP_REMOVED]);
    assert!(list_incoming_is_empty(&env, &bob).await);

    // Gone is gone: a second delete is a 404, and they can become friends again by request.
    let again = call(
        &env,
        "DELETE",
        &format!("/v1/friends/{}", bob.id),
        &alice,
        None,
    )
    .await;
    assert_problem(again, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    let token = invite_token(&env, &bob).await;
    assert_eq!(
        request_via_invite(&env, &alice, &token).await.status(),
        StatusCode::CREATED
    );
}

async fn list_incoming_is_empty(env: &TestEnv, user: &User) -> bool {
    let response = call(
        env,
        "GET",
        "/v1/friend-requests?direction=incoming",
        user,
        None,
    )
    .await;
    json_body(response).await["items"]
        .as_array()
        .unwrap()
        .is_empty()
}

#[tokio::test]
async fn an_edge_that_does_not_involve_the_caller_is_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let mallory = adult(&env).await;
    make_friends(&env, alice.id, bob.id).await;

    let request_id = Uuid::now_v7();
    for target in [alice.id, bob.id] {
        let denied = send(
            &env.router,
            delete_request(&mallory, &format!("/v1/friends/{target}"), request_id),
        )
        .await;
        assert_eq!(denied.status(), StatusCode::NOT_FOUND);
        let missing = send(
            &env.router,
            delete_request(
                &mallory,
                &format!("/v1/friends/{}", Uuid::now_v7()),
                request_id,
            ),
        )
        .await;
        assert_eq!(body_bytes(denied).await, body_bytes(missing).await);
    }
    let malformed = call(&env, "DELETE", "/v1/friends/not-an-id", &mallory, None).await;
    assert_problem(malformed, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    assert_eq!(friend_ids(&env, &alice).await, [bob.id.to_string()]);
}
