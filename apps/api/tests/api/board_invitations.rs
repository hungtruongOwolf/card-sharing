//! Board invitations (T-106, AC-19.1, AC-19.8, SR-21): only current friends, members plus open
//! invitations within 8, membership only after acceptance, silent declines and drops.

use crate::boards::new_board;
use crate::common::*;
use crate::media::without_request_id;
use crate::social_support::*;
use axum::body::Body;
use http::{Request, Response, StatusCode};
use serde_json::{Value, json};
use time::Duration;
use uuid::Uuid;

pub async fn invite(env: &TestEnv, owner: &User, board: Uuid, user: Uuid) -> Response<Body> {
    let request = Request::post(format!("/v1/boards/{board}/invitations"))
        .header("authorization", format!("Bearer {}", owner.token))
        .header("content-type", "application/json")
        .header("idempotency-key", Uuid::now_v7().to_string())
        .body(Body::from(json!({ "user_id": user }).to_string()))
        .unwrap();
    send(&env.router, request).await
}

/// Invites `user` and returns the invitation id.
pub async fn invited(env: &TestEnv, owner: &User, board: Uuid, user: &User) -> Uuid {
    let response = invite(env, owner, board, user.id).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = json_body(response).await;
    assert_eq!(body["invitation"]["status"], "pending");
    Uuid::parse_str(body["invitation"]["id"].as_str().unwrap()).unwrap()
}

pub async fn accept(env: &TestEnv, user: &User, invitation: Uuid) -> Response<Body> {
    call(
        env,
        "POST",
        &format!("/v1/board-invitations/{invitation}/accept"),
        user,
        None,
    )
    .await
}

/// Invites and accepts: `user` becomes a collaborator through the real flow.
pub async fn join(env: &TestEnv, owner: &User, board: Uuid, user: &User) {
    let invitation = invited(env, owner, board, user).await;
    assert_eq!(accept(env, user, invitation).await.status(), StatusCode::OK);
}

async fn mine(env: &TestEnv, user: &User) -> Vec<Value> {
    let response = call(env, "GET", "/v1/me/board-invitations", user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["items"]
        .as_array()
        .unwrap()
        .clone()
}

async fn owner_list(env: &TestEnv, owner: &User, board: Uuid) -> Vec<Value> {
    let response = call(
        env,
        "GET",
        &format!("/v1/boards/{board}/invitations"),
        owner,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["items"]
        .as_array()
        .unwrap()
        .clone()
}

async fn board_status(env: &TestEnv, user: &User, board: Uuid) -> StatusCode {
    call(env, "GET", &format!("/v1/boards/{board}"), user, None)
        .await
        .status()
}

/// An owner with a board and a friend.
async fn owner_and_friend(env: &TestEnv) -> (User, User, Uuid) {
    let owner = adult(env).await;
    let friend = adult(env).await;
    make_friends(env, owner.id, friend.id).await;
    let board = new_board(env, &owner).await;
    (owner, friend, board)
}

#[tokio::test]
async fn membership_starts_only_when_the_invitee_accepts() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (owner, friend, board) = owner_and_friend(&env).await;
    let invitation = invited(&env, &owner, board, &friend).await;

    // AC-19.8: invited is not a member; the invitee sees no member list before accepting.
    assert_eq!(
        board_status(&env, &friend, board).await,
        StatusCode::NOT_FOUND
    );
    let received = mine(&env, &friend).await;
    assert_eq!(received.len(), 1);
    assert_eq!(received[0]["board_title"], "Summer");
    assert_eq!(received[0]["owner_display_name"], owner.name);
    assert_eq!(received[0]["member_count"], 1);
    assert!(received[0].get("members").is_none());
    let members = call(
        &env,
        "GET",
        &format!("/v1/boards/{board}/members"),
        &friend,
        None,
    )
    .await;
    assert_eq!(members.status(), StatusCode::NOT_FOUND);
    assert_eq!(owner_list(&env, &owner, board).await.len(), 1);

    // Someone else cannot accept or decline it.
    let other = adult(&env).await;
    let denied = accept(&env, &other, invitation).await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    let denied = without_request_id(body_bytes(denied).await);
    let missing = accept(&env, &other, Uuid::now_v7()).await;
    assert_eq!(without_request_id(body_bytes(missing).await), denied);
    let decline = call(
        &env,
        "POST",
        &format!("/v1/board-invitations/{invitation}/decline"),
        &other,
        None,
    )
    .await;
    assert_eq!(decline.status(), StatusCode::NOT_FOUND);

    let accepted = accept(&env, &friend, invitation).await;
    assert_eq!(accepted.status(), StatusCode::OK);
    assert_eq!(json_body(accepted).await["board_id"], board.to_string());
    assert_eq!(board_status(&env, &friend, board).await, StatusCode::OK);
    assert!(mine(&env, &friend).await.is_empty());
    assert!(owner_list(&env, &owner, board).await.is_empty());
    // Accepting twice is the missing 404; inviting a member is a conflict.
    assert_eq!(
        accept(&env, &friend, invitation).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_problem(
        invite(&env, &owner, board, friend.id).await,
        StatusCode::CONFLICT,
        "ALREADY_INVITED",
    )
    .await;
}

#[tokio::test]
async fn only_the_owner_invites_and_only_current_friends() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (owner, friend, board) = owner_and_friend(&env).await;
    let stranger = adult(&env).await;
    let missing =
        without_request_id(body_bytes(invite(&env, &owner, Uuid::now_v7(), friend.id).await).await);
    // AC-19.1: a non-friend target is the missing 404, like a missing board.
    let denied = invite(&env, &owner, board, stranger.id).await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    assert_eq!(without_request_id(body_bytes(denied).await), missing);
    assert_eq!(
        invite(&env, &owner, board, owner.id).await.status(),
        StatusCode::NOT_FOUND
    );
    // A collaborator cannot invite, nor list or cancel the owner's invitations.
    join(&env, &owner, board, &friend).await;
    let friends_friend = adult(&env).await;
    make_friends(&env, friend.id, friends_friend.id).await;
    make_friends(&env, owner.id, friends_friend.id).await;
    assert_eq!(
        invite(&env, &friend, board, friends_friend.id)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let invitation = invited(&env, &owner, board, &friends_friend).await;
    let listed = call(
        &env,
        "GET",
        &format!("/v1/boards/{board}/invitations"),
        &friend,
        None,
    )
    .await;
    assert_eq!(listed.status(), StatusCode::NOT_FOUND);
    let cancel = call(
        &env,
        "DELETE",
        &format!("/v1/board-invitations/{invitation}"),
        &friend,
        None,
    )
    .await;
    assert_eq!(cancel.status(), StatusCode::NOT_FOUND);
    assert_problem(
        invite(&env, &owner, board, friends_friend.id).await,
        StatusCode::CONFLICT,
        "ALREADY_INVITED",
    )
    .await;
    // The owner cancels; the invitee no longer sees it and cannot accept it.
    let cancel = call(
        &env,
        "DELETE",
        &format!("/v1/board-invitations/{invitation}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(cancel.status(), StatusCode::NO_CONTENT);
    assert!(mine(&env, &friends_friend).await.is_empty());
    assert_eq!(
        accept(&env, &friends_friend, invitation).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn members_plus_open_invitations_stay_within_eight() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let board = new_board(&env, &owner).await;
    let mut friends = Vec::new();
    for _ in 0..9 {
        let friend = adult(&env).await;
        make_friends(&env, owner.id, friend.id).await;
        friends.push(friend);
    }
    join(&env, &owner, board, &friends[0]).await;
    let mut invitations = Vec::new();
    for friend in &friends[1..8] {
        invitations.push(invited(&env, &owner, board, friend).await);
    }
    assert_problem(
        invite(&env, &owner, board, friends[8].id).await,
        StatusCode::CONFLICT,
        "COLLABORATOR_LIMIT_REACHED",
    )
    .await;
    // A declined invitation still holds its seat until it expires (declines are silent).
    let decline = call(
        &env,
        "POST",
        &format!("/v1/board-invitations/{}/decline", invitations[0]),
        &friends[1],
        None,
    )
    .await;
    assert_eq!(decline.status(), StatusCode::NO_CONTENT);
    assert_eq!(owner_list(&env, &owner, board).await.len(), 7);
    assert_problem(
        invite(&env, &owner, board, friends[8].id).await,
        StatusCode::CONFLICT,
        "COLLABORATOR_LIMIT_REACHED",
    )
    .await;
    let cancel = call(
        &env,
        "DELETE",
        &format!("/v1/board-invitations/{}", invitations[1]),
        &owner,
        None,
    )
    .await;
    assert_eq!(cancel.status(), StatusCode::NO_CONTENT);
    invited(&env, &owner, board, &friends[8]).await;
}

#[tokio::test]
async fn declines_drops_and_expiry_look_the_same_to_the_owner() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (owner, friend, board) = owner_and_friend(&env).await;
    let invitation = invited(&env, &owner, board, &friend).await;
    let decline = call(
        &env,
        "POST",
        &format!("/v1/board-invitations/{invitation}/decline"),
        &friend,
        None,
    )
    .await;
    assert_eq!(decline.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        owner_list(&env, &owner, board).await[0]["status"],
        "pending"
    );
    assert_eq!(
        accept(&env, &friend, invitation).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_problem(
        invite(&env, &owner, board, friend.id).await,
        StatusCode::CONFLICT,
        "ALREADY_INVITED",
    )
    .await;

    // After expiry, re-inviting waits for the cooldown, then works.
    env.clock.advance(Duration::days(8));
    // Access tokens live 10 minutes: sign in again after moving the clock.
    let owner = signed_in_again(&env, &owner).await;
    let friend = signed_in_again(&env, &friend).await;
    assert!(owner_list(&env, &owner, board).await.is_empty());
    let cooling = invite(&env, &owner, board, friend.id).await;
    assert_eq!(cooling.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(cooling.headers().contains_key("retry-after"));
    assert_problem(cooling, StatusCode::TOO_MANY_REQUESTS, "REINVITE_COOLDOWN").await;
    env.clock.advance(Duration::days(7));
    let owner = signed_in_again(&env, &owner).await;
    invited(&env, &owner, board, &friend).await;

    // A block between the invitee and any member: same 201, nothing reaches the invitee.
    let collaborator = adult(&env).await;
    make_friends(&env, owner.id, collaborator.id).await;
    join(&env, &owner, board, &collaborator).await;
    let blocked = adult(&env).await;
    make_friends(&env, owner.id, blocked.id).await;
    block(&env, collaborator.id, blocked.id).await;
    invited(&env, &owner, board, &blocked).await;
    assert!(mine(&env, &blocked).await.is_empty());
    assert_eq!(owner_list(&env, &owner, board).await.len(), 2);
}

#[tokio::test]
async fn accepting_rechecks_friendship_blocks_and_expiry() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (owner, friend, board) = owner_and_friend(&env).await;
    let unfriended = invited(&env, &owner, board, &friend).await;
    sqlx::query("DELETE FROM social.friendships WHERE user_a = LEAST($1::uuid, $2::uuid) AND user_b = GREATEST($1::uuid, $2::uuid)")
        .bind(owner.id)
        .bind(friend.id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(
        accept(&env, &friend, unfriended).await.status(),
        StatusCode::NOT_FOUND
    );

    let member = adult(&env).await;
    let later_blocked = adult(&env).await;
    make_friends(&env, owner.id, member.id).await;
    make_friends(&env, owner.id, later_blocked.id).await;
    join(&env, &owner, board, &member).await;
    let invitation = invited(&env, &owner, board, &later_blocked).await;
    block(&env, later_blocked.id, member.id).await;
    assert_eq!(
        accept(&env, &later_blocked, invitation).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        board_status(&env, &later_blocked, board).await,
        StatusCode::NOT_FOUND
    );

    let late = adult(&env).await;
    make_friends(&env, owner.id, late.id).await;
    let expired = invited(&env, &owner, board, &late).await;
    env.clock.advance(Duration::days(7) + Duration::seconds(1));
    let late = signed_in_again(&env, &late).await;
    assert!(mine(&env, &late).await.is_empty());
    assert_eq!(
        accept(&env, &late, expired).await.status(),
        StatusCode::NOT_FOUND
    );
}
