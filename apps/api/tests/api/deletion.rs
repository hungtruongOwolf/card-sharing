//! Account deletion with a 7-day grace period (T-151, US-29 AC-29.1..29.3): fresh re-auth,
//! logout everywhere, content hidden at once, shared boards deleted, pins on other boards removed,
//! cancel by signing in again within the grace period.

use crate::board_invitations::join;
use crate::board_ops::{add_card, apply};
use crate::board_read::read_board;
use crate::boards::{assert_board_hidden, new_board};
use crate::cards_read::deliver_to;
use crate::common::*;
use crate::public_links::created_link;
use crate::reauth::{reauth_token, subject_of};
use crate::social_support::*;
use axum::body::Body;
use http::{Request, Response, StatusCode};
use identity::testing::random_nonce;
use serde_json::{Value, json};
use time::Duration;
use uuid::Uuid;

pub async fn request_deletion(env: &TestEnv, user: &User, reauth: Option<&str>) -> Response<Body> {
    let mut builder =
        Request::post("/v1/me/deletion").header("authorization", format!("Bearer {}", user.token));
    if let Some(reauth) = reauth {
        builder = builder.header("x-reauth", reauth);
    }
    send(&env.router, builder.body(Body::empty()).unwrap()).await
}

async fn feed_len(env: &TestEnv, user: &User) -> usize {
    let response = call(env, "GET", "/v1/feed", user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["items"].as_array().unwrap().len()
}

/// Signs `user`'s Apple identity in again and returns the new session as a `User`.
async fn sign_in_again(env: &TestEnv, user: &User) -> User {
    let subject = subject_of(env, user).await;
    let nonce = random_nonce();
    let body = json!({ "id_token": env.apple_id_token(&subject, &nonce), "nonce": nonce });
    let response = send(&env.router, post_json("/v1/auth/apple", None, &body)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tokens = json_body(response).await;
    User {
        token: tokens["access_token"].as_str().unwrap().to_owned(),
        ..user.clone()
    }
}

struct World {
    me: User,
    friend: User,
    shared_board: Uuid,
    personal_board: Uuid,
    their_board: Uuid,
}

async fn world(env: &TestEnv) -> World {
    let me = adult(env).await;
    let friend = adult(env).await;
    make_friends(env, me.id, friend.id).await;
    let card = deliver_to(env, &me, &[&friend]).await;
    let shared_board = new_board(env, &me).await;
    join(env, &me, shared_board, &friend).await;
    let personal_board = new_board(env, &me).await;
    let their_board = new_board(env, &friend).await;
    join(env, &friend, their_board, &me).await;
    apply(
        env,
        &me,
        their_board,
        json!([add_card(Uuid::now_v7(), card)]),
    )
    .await;
    created_link(env, &me, personal_board, "never").await;
    World {
        me,
        friend,
        shared_board,
        personal_board,
        their_board,
    }
}

#[tokio::test]
async fn deletion_needs_a_fresh_reauth() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let response = request_deletion(&env, &user, None).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "REAUTH_REQUIRED").await;
    let response = request_deletion(&env, &user, Some("forged")).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "REAUTH_REQUIRED").await;
    // Another account's re-auth token does not work either.
    let other = adult(&env).await;
    let theirs = reauth_token(&env, &other).await;
    let response = request_deletion(&env, &user, Some(&theirs)).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "REAUTH_REQUIRED").await;
    let status: String = sqlx::query_scalar("SELECT status FROM identity.accounts WHERE id = $1")
        .bind(user.id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(status, "active");
}

#[tokio::test]
async fn requesting_logs_out_everywhere_and_hides_content_at_once() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let w = world(&env).await;
    let other_device = signed_in_again(&env, &w.me).await;
    assert_eq!(feed_len(&env, &w.friend).await, 1);

    let reauth = reauth_token(&env, &w.me).await;
    let response = request_deletion(&env, &w.me, Some(&reauth)).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = json_body(response).await;
    assert_eq!(body["deletion"]["status"], "pending");
    let parse = |v: &Value| {
        time::OffsetDateTime::parse(
            v.as_str().unwrap(),
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap()
    };
    assert_eq!(
        parse(&body["deletion"]["grace_until"]) - parse(&body["deletion"]["requested_at"]),
        Duration::days(7)
    );

    // AC-29.1: every device is logged out.
    for session in [&w.me, &other_device] {
        let me = call(&env, "GET", "/v1/me", session, None).await;
        assert_eq!(me.status(), StatusCode::UNAUTHORIZED);
    }
    // Content hidden from friends at once.
    assert_eq!(feed_len(&env, &w.friend).await, 0);
    // AC-29.3: the shared board is gone; the personal one is only hidden (erased later).
    assert_board_hidden(&env, &w.friend, w.shared_board).await;
    let deleted: Option<time::OffsetDateTime> =
        sqlx::query_scalar("SELECT deleted_at FROM boards.boards WHERE id = $1")
            .bind(w.personal_board)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert!(deleted.is_none());
    // Its pin on the friend's board is removed and it is no longer a member there.
    let board = read_board(&env, &w.friend, w.their_board, "").await;
    assert!(board["pins"].as_array().unwrap().is_empty(), "{board}");
    let members: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM boards.board_members WHERE board_id = $1 AND user_id = $2",
    )
    .bind(w.their_board)
    .bind(w.me.id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(members, 0);
    // Public links revoked.
    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sharing.public_links WHERE owner_id = $1 AND revoked_at IS NULL",
    )
    .bind(w.me.id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(live, 0);
}

#[tokio::test]
async fn signing_in_during_the_grace_period_can_only_see_and_cancel() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let w = world(&env).await;
    let reauth = reauth_token(&env, &w.me).await;
    assert_eq!(
        request_deletion(&env, &w.me, Some(&reauth)).await.status(),
        StatusCode::ACCEPTED
    );
    let back = sign_in_again(&env, &w.me).await;
    let me = call(&env, "GET", "/v1/me", &back, None).await;
    assert_eq!(me.status(), StatusCode::OK);
    assert_eq!(json_body(me).await["account"]["status"], "deletion_pending");
    let pending = call(&env, "GET", "/v1/me/deletion", &back, None).await;
    assert_eq!(pending.status(), StatusCode::OK);
    // Nothing else is reachable while the deletion is pending.
    let feed = call(&env, "GET", "/v1/feed", &back, None).await;
    assert_problem(feed, StatusCode::UNAUTHORIZED, "SESSION_REVOKED").await;

    let cancelled = call(&env, "DELETE", "/v1/me/deletion", &back, None).await;
    assert_eq!(cancelled.status(), StatusCode::NO_CONTENT);
    let me = call(&env, "GET", "/v1/me", &back, None).await;
    assert_eq!(json_body(me).await["account"]["status"], "active");
    assert_eq!(
        feed_len(&env, &back).await,
        0,
        "the account's own feed works again"
    );
    assert_eq!(
        feed_len(&env, &w.friend).await,
        1,
        "the card is visible again"
    );
    let again = call(&env, "DELETE", "/v1/me/deletion", &back, None).await;
    assert_problem(again, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    let none = call(&env, "GET", "/v1/me/deletion", &back, None).await;
    assert_problem(none, StatusCode::NOT_FOUND, "NOT_FOUND").await;
}

#[tokio::test]
async fn after_the_grace_period_the_deletion_cannot_be_cancelled() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let reauth = reauth_token(&env, &user).await;
    assert_eq!(
        request_deletion(&env, &user, Some(&reauth)).await.status(),
        StatusCode::ACCEPTED
    );
    env.clock.advance(Duration::days(7) + Duration::seconds(1));
    let back = sign_in_again(&env, &user).await;
    let cancel = call(&env, "DELETE", "/v1/me/deletion", &back, None).await;
    assert_problem(cancel, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    let status: String = sqlx::query_scalar("SELECT status FROM identity.accounts WHERE id = $1")
        .bind(user.id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(status, "deletion_pending");
}
