//! T-39: invite preview and friend requests from an invite (US-5 AC-5.1, SR-20, SR-22), against
//! real Postgres + Redis.

use crate::common::*;
use crate::social_support::*;
use axum::body::Body;
use http::{Request, StatusCode};
use serde_json::{Value, json};
use social::InviteError;
use uuid::Uuid;

fn preview_request(user: &User, token: &str, request_id: Uuid) -> Request<Body> {
    Request::get(format!("/v1/invites/{token}"))
        .header("authorization", format!("Bearer {}", user.token))
        .header("x-request-id", request_id.to_string())
        .body(Body::empty())
        .unwrap()
}

async fn created(response: http::Response<Body>) -> Value {
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = json_body(response).await;
    assert_eq!(body["request"]["status"], "pending");
    body
}

async fn friendship_exists(env: &TestEnv, a: Uuid, b: Uuid) -> bool {
    let (low, high) = if a < b { (a, b) } else { (b, a) };
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM social.friendships WHERE user_a = $1 AND user_b = $2)",
    )
    .bind(low)
    .bind(high)
    .fetch_one(env.identity.pool())
    .await
    .unwrap()
}

async fn events_for(env: &TestEnv, request_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM platform.outbox_events
         WHERE topic = 'social.friend_request_created' AND payload->>'request_id' = $1",
    )
    .bind(request_id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn preview_shows_the_display_name_only_and_flags_your_own_invite() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let token = invite_token(&env, &alice).await;

    let response = send(&env.router, preview_request(&bob, &token, Uuid::now_v7())).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(
        body,
        json!({ "inviter": { "display_name": alice.name }, "is_self": false })
    );

    let own =
        json_body(send(&env.router, preview_request(&alice, &token, Uuid::now_v7())).await).await;
    assert_eq!(own["is_self"], true);
}

#[tokio::test]
async fn a_minors_invite_preview_has_no_avatar() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let teen = minor(&env).await;
    let viewer = adult(&env).await;
    let token = invite_token(&env, &teen).await;
    let body = json_body(
        send(
            &env.router,
            preview_request(&viewer, &token, Uuid::now_v7()),
        )
        .await,
    )
    .await;
    let inviter: Vec<&String> = body["inviter"].as_object().unwrap().keys().collect();
    assert_eq!(inviter, ["display_name"]);
}

#[tokio::test]
async fn unknown_blocked_and_expired_tokens_answer_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let token = invite_token(&env, &alice).await;
    block(&env, alice.id, bob.id).await;

    // A block answers byte-for-byte like a token that never existed.
    let request_id = Uuid::now_v7();
    let blocked = send(&env.router, preview_request(&bob, &token, request_id)).await;
    assert_eq!(blocked.status(), StatusCode::NOT_FOUND);
    let unknown = send(
        &env.router,
        preview_request(&bob, "ZZZZZZZZZZZZZZZZZZZZZZ", request_id),
    )
    .await;
    assert_eq!(body_bytes(blocked).await, body_bytes(unknown).await);

    // The other direction too.
    let carol = adult(&env).await;
    block(&env, carol.id, alice.id).await;
    let response = send(&env.router, preview_request(&carol, &token, Uuid::now_v7())).await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;

    // A reset token is expired, not unknown.
    let dave = adult(&env).await;
    call(&env, "POST", "/v1/me/invite/reset", &alice, None).await;
    let response = send(&env.router, preview_request(&dave, &token, Uuid::now_v7())).await;
    assert_problem(response, StatusCode::NOT_FOUND, "INVITE_EXPIRED").await;
}

#[tokio::test]
async fn a_suspended_inviter_looks_unknown() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let token = invite_token(&env, &alice).await;
    sqlx::query("UPDATE identity.accounts SET status = 'suspended' WHERE id = $1")
        .bind(alice.id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let response = send(&env.router, preview_request(&bob, &token, Uuid::now_v7())).await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    assert_problem(
        request_via_invite(&env, &bob, &token).await,
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
    )
    .await;
}

#[tokio::test]
async fn random_tokens_are_generic_404s_and_get_throttled() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let prober = adult(&env).await;
    // 10k random well-formed tokens: none resolves.
    for _ in 0..10_000 {
        let token = &base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            Uuid::now_v7().as_bytes(),
        );
        assert!(matches!(
            env.state.social.resolve_invite(token).await,
            Err(InviteError::NotFound)
        ));
    }
    // Over HTTP the per-IP limit (30/min) stops a prober quickly.
    let request_id = Uuid::now_v7();
    let mut first_body = None;
    let mut throttled = 0;
    for i in 0..40 {
        let token = format!("{:A>22}", i);
        let response = send(&env.router, preview_request(&prober, &token, request_id)).await;
        match response.status() {
            StatusCode::NOT_FOUND => {
                let body = body_bytes(response).await;
                if let Some(first) = &first_body {
                    assert_eq!(first, &body, "every miss is byte-identical");
                } else {
                    first_body = Some(body);
                }
            }
            StatusCode::TOO_MANY_REQUESTS => throttled += 1,
            other => panic!("unexpected {other}"),
        }
    }
    // 30 pass, the rest are throttled (a token may refill while the loop runs).
    assert!(throttled >= 9, "{throttled}");
}

#[tokio::test]
async fn opening_an_invite_sends_a_request_but_no_friendship() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let token = invite_token(&env, &alice).await;

    let body = created(request_via_invite(&env, &bob, &token).await).await;
    let id = body["request"]["id"].as_str().unwrap().to_owned();
    assert_eq!(request_status(&env, &id).await, "pending");
    assert!(!friendship_exists(&env, alice.id, bob.id).await, "AC-5.1");
    assert_eq!(events_for(&env, &id).await, 1);

    // Asking again returns the open request (pair dedupe), no second event.
    let again = created(request_via_invite(&env, &bob, &token).await).await;
    assert_eq!(again["request"]["id"], id.as_str());
    assert_eq!(events_for(&env, &id).await, 1);
}

#[tokio::test]
async fn your_own_invite_cannot_send_a_request() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let token = invite_token(&env, &alice).await;
    assert_problem(
        request_via_invite(&env, &alice, &token).await,
        StatusCode::BAD_REQUEST,
        "VALIDATION_FAILED",
    )
    .await;
}

#[tokio::test]
async fn blocked_pairs_get_the_same_201_and_nothing_is_delivered() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    for inviter_blocks in [true, false] {
        let alice = adult(&env).await;
        let bob = adult(&env).await;
        let token = invite_token(&env, &alice).await;
        if inviter_blocks {
            block(&env, alice.id, bob.id).await;
        } else {
            block(&env, bob.id, alice.id).await;
        }
        let body = created(request_via_invite(&env, &bob, &token).await).await;
        let keys: Vec<&String> = body["request"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["expires_at", "id", "status"]);
        let id = body["request"]["id"].as_str().unwrap();
        assert_eq!(request_status(&env, id).await, "dropped");
        assert_eq!(
            events_for(&env, id).await,
            0,
            "no push for a dropped request"
        );
    }
}

#[tokio::test]
async fn idempotency_key_is_required_and_bodies_are_allow_lists() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let token = invite_token(&env, &alice).await;
    let no_key = json_request(
        "POST",
        "/v1/friend-requests",
        Some(&bob.token),
        Some(&json!({ "invite_token": token })),
    );
    assert_problem(
        send(&env.router, no_key).await,
        StatusCode::BAD_REQUEST,
        "VALIDATION_FAILED",
    )
    .await;
    let extra = Request::post("/v1/friend-requests")
        .header("authorization", format!("Bearer {}", bob.token))
        .header("content-type", "application/json")
        .header("idempotency-key", Uuid::now_v7().to_string())
        .body(Body::from(
            json!({ "invite_token": token, "from_user": alice.id }).to_string(),
        ))
        .unwrap();
    assert_problem(
        send(&env.router, extra).await,
        StatusCode::BAD_REQUEST,
        "VALIDATION_FAILED",
    )
    .await;
}

#[tokio::test]
async fn a_minors_invite_counts_uses_inside_the_request() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let cap = policy_service()
        .resolve(&Default::default())
        .invites
        .minor_max_uses;
    let teen = minor(&env).await;
    let token = invite_token(&env, &teen).await;
    for _ in 0..cap {
        let friend = adult(&env).await;
        created(request_via_invite(&env, &friend, &token).await).await;
    }
    let late = adult(&env).await;
    assert_problem(
        request_via_invite(&env, &late, &token).await,
        StatusCode::NOT_FOUND,
        "INVITE_EXPIRED",
    )
    .await;
}

#[tokio::test]
async fn friends_and_full_friend_lists_are_refused() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let token = invite_token(&env, &alice).await;
    make_friends(&env, alice.id, bob.id).await;
    assert_problem(
        request_via_invite(&env, &bob, &token).await,
        StatusCode::CONFLICT,
        "ALREADY_FRIENDS",
    )
    .await;

    // Alice has 20 friends: a new request is refused (AC-5.2).
    for _ in 0..19 {
        let other = adult(&env).await;
        make_friends(&env, alice.id, other.id).await;
    }
    let carol = adult(&env).await;
    assert_problem(
        request_via_invite(&env, &carol, &token).await,
        StatusCode::CONFLICT,
        "FRIEND_LIMIT_REACHED",
    )
    .await;
}
