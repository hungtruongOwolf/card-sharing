//! T-40: list, accept, decline and cancel friend requests (US-5 AC-5.2, SR-22) against real
//! Postgres + Redis.

use crate::common::*;
use crate::social_support::*;
use axum::body::Body;
use http::{Request, StatusCode};
use serde_json::Value;
use uuid::Uuid;

/// `from` sends a request to `to` through `to`'s invite; returns the request id.
async fn send_request(env: &TestEnv, from: &User, to: &User) -> String {
    let token = invite_token(env, to).await;
    let response = request_via_invite(env, from, &token).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await["request"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn list(env: &TestEnv, user: &User, direction: &str) -> Vec<Value> {
    let path = format!("/v1/friend-requests?direction={direction}");
    let response = call(env, "GET", &path, user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["items"]
        .as_array()
        .unwrap()
        .clone()
}

async fn answer(env: &TestEnv, user: &User, method: &str, path: &str) -> http::Response<Body> {
    call(env, method, path, user, None).await
}

async fn friend_count(env: &TestEnv, id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM social.friendships WHERE user_a = $1 OR user_b = $1")
        .bind(id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn both_sides_see_the_request_until_it_is_accepted() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let id = send_request(&env, &bob, &alice).await;

    let incoming = list(&env, &alice, "incoming").await;
    assert_eq!(incoming.len(), 1);
    assert_eq!(incoming[0]["id"], id.as_str());
    assert_eq!(incoming[0]["user"]["id"], bob.id.to_string());
    assert_eq!(incoming[0]["user"]["display_name"], bob.name.as_str());
    assert_eq!(incoming[0]["user"]["handle"], bob.handle.as_str());
    let outgoing = list(&env, &bob, "outgoing").await;
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0]["user"]["id"], alice.id.to_string());
    assert!(list(&env, &alice, "outgoing").await.is_empty());

    let response = answer(
        &env,
        &alice,
        "POST",
        &format!("/v1/friend-requests/{id}/accept"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["friend"]["id"],
        bob.id.to_string()
    );
    assert_eq!(friend_count(&env, alice.id).await, 1);
    assert!(list(&env, &alice, "incoming").await.is_empty());
    assert!(list(&env, &bob, "outgoing").await.is_empty());
    assert_eq!(request_status(&env, &id).await, "accepted");
}

#[tokio::test]
async fn accepting_also_answers_the_request_in_the_other_direction() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let to_alice = send_request(&env, &bob, &alice).await;
    let to_bob = send_request(&env, &alice, &bob).await;
    let response = answer(
        &env,
        &alice,
        "POST",
        &format!("/v1/friend-requests/{to_alice}/accept"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(request_status(&env, &to_bob).await, "accepted");
    assert!(list(&env, &bob, "incoming").await.is_empty());
}

#[tokio::test]
async fn either_side_at_twenty_friends_cannot_accept() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    for full_side_is_recipient in [true, false] {
        let alice = adult(&env).await;
        let bob = adult(&env).await;
        let id = send_request(&env, &bob, &alice).await;
        let full = if full_side_is_recipient { &alice } else { &bob };
        for _ in 0..20 {
            let other = adult(&env).await;
            make_friends(&env, full.id, other.id).await;
        }
        let response = answer(
            &env,
            &alice,
            "POST",
            &format!("/v1/friend-requests/{id}/accept"),
        )
        .await;
        assert_problem(response, StatusCode::CONFLICT, "FRIEND_LIMIT_REACHED").await;
        assert_eq!(request_status(&env, &id).await, "pending", "stays pending");
    }
}

#[tokio::test]
async fn requests_of_other_users_are_404_like_missing_ids() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let carol = adult(&env).await;
    let id = send_request(&env, &bob, &alice).await;
    let missing = Uuid::now_v7();

    let cases = [
        // Someone else accepting / declining a request addressed to Alice.
        (
            &carol,
            "POST",
            format!("/v1/friend-requests/{id}/accept"),
            format!("/v1/friend-requests/{missing}/accept"),
        ),
        (
            &carol,
            "POST",
            format!("/v1/friend-requests/{id}/decline"),
            format!("/v1/friend-requests/{missing}/decline"),
        ),
        // The sender cannot accept their own request.
        (
            &bob,
            "POST",
            format!("/v1/friend-requests/{id}/accept"),
            format!("/v1/friend-requests/{missing}/accept"),
        ),
        // Only the sender can cancel.
        (
            &carol,
            "DELETE",
            format!("/v1/friend-requests/{id}"),
            format!("/v1/friend-requests/{missing}"),
        ),
        (
            &alice,
            "DELETE",
            format!("/v1/friend-requests/{id}"),
            format!("/v1/friend-requests/{missing}"),
        ),
    ];
    for (user, method, path, missing_path) in cases {
        let request_id = Uuid::now_v7();
        let denied = send(
            &env.router,
            with_request_id(method, &path, user, request_id),
        )
        .await;
        assert_eq!(denied.status(), StatusCode::NOT_FOUND, "{method} {path}");
        let absent = send(
            &env.router,
            with_request_id(method, &missing_path, user, request_id),
        )
        .await;
        assert_eq!(
            body_bytes(denied).await,
            body_bytes(absent).await,
            "{method} {path}"
        );
    }
    let malformed = answer(
        &env,
        &alice,
        "POST",
        "/v1/friend-requests/not-a-uuid/accept",
    )
    .await;
    assert_problem(malformed, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    assert_eq!(request_status(&env, &id).await, "pending");
}

fn with_request_id(method: &str, path: &str, user: &User, request_id: Uuid) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {}", user.token))
        .header("x-request-id", request_id.to_string())
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn a_decline_is_silent_to_the_sender() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let id = send_request(&env, &bob, &alice).await;
    let response = answer(
        &env,
        &alice,
        "POST",
        &format!("/v1/friend-requests/{id}/decline"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(list(&env, &alice, "incoming").await.is_empty());
    let outgoing = list(&env, &bob, "outgoing").await;
    assert_eq!(outgoing.len(), 1, "the sender still sees it as pending");
    // Asking again does not reach Alice either: the open request is returned.
    let again = send_request(&env, &bob, &alice).await;
    assert_eq!(again, id);
    assert!(list(&env, &alice, "incoming").await.is_empty());
    // A declined request cannot be accepted later.
    let response = answer(
        &env,
        &alice,
        "POST",
        &format!("/v1/friend-requests/{id}/accept"),
    )
    .await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
}

#[tokio::test]
async fn the_sender_can_cancel() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let id = send_request(&env, &bob, &alice).await;
    let response = answer(&env, &bob, "DELETE", &format!("/v1/friend-requests/{id}")).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(list(&env, &alice, "incoming").await.is_empty());
    assert!(list(&env, &bob, "outgoing").await.is_empty());
    let response = answer(
        &env,
        &alice,
        "POST",
        &format!("/v1/friend-requests/{id}/accept"),
    )
    .await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
}

#[tokio::test]
async fn direction_is_validated() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    for path in [
        "/v1/friend-requests",
        "/v1/friend-requests?direction=sideways",
        "/v1/friend-requests?direction=incoming&user_id=x",
    ] {
        let response = call(&env, "GET", path, &alice, None).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
}

#[tokio::test]
async fn concurrent_accepts_never_exceed_twenty_friends() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    for _ in 0..19 {
        let other = adult(&env).await;
        make_friends(&env, alice.id, other.id).await;
    }
    let mut ids = Vec::new();
    for _ in 0..6 {
        let sender = adult(&env).await;
        ids.push(send_request(&env, &sender, &alice).await);
    }
    let mut tasks = Vec::new();
    for id in ids {
        let router = env.router.clone();
        let token = alice.token.clone();
        tasks.push(tokio::spawn(async move {
            let request = Request::post(format!("/v1/friend-requests/{id}/accept"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap();
            send(&router, request).await.status()
        }));
    }
    let mut accepted = 0;
    for task in tasks {
        match task.await.unwrap() {
            StatusCode::OK => accepted += 1,
            StatusCode::CONFLICT => {}
            other => panic!("unexpected {other}"),
        }
    }
    assert_eq!(accepted, 1);
    assert_eq!(friend_count(&env, alice.id).await, 20);
}
