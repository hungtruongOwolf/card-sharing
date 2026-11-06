//! Leaving and removal (T-107, AC-19.6, SR-7): the owner removes collaborators, collaborators
//! leave; a removed member's pins go with them and their writes are refused. A block between two
//! members ends the co-membership.

use crate::board_invitations::{invited, join};
use crate::board_ops::{add_card, apply, post_ops, remove};
use crate::board_read::{pin_of, read_board};
use crate::boards::new_board;
use crate::cards_read::deliver_to;
use crate::common::*;
use crate::media::without_request_id;
use crate::social_support::*;
use http::StatusCode;
use platform::clock::Clock;
use platform::outbox::Subscriber;
use serde_json::json;
use uuid::Uuid;

async fn remove_member(
    env: &TestEnv,
    caller: &User,
    board: Uuid,
    user: Uuid,
) -> http::Response<axum::body::Body> {
    call(
        env,
        "DELETE",
        &format!("/v1/boards/{board}/members/{user}"),
        caller,
        None,
    )
    .await
}

async fn status_of(env: &TestEnv, user: &User, board: Uuid) -> StatusCode {
    call(env, "GET", &format!("/v1/boards/{board}"), user, None)
        .await
        .status()
}

fn resolver(env: &TestEnv) -> boards::BlockResolver {
    boards::BlockResolver::new(env.identity.pool().clone(), env.clock.clone())
}

/// An owner with a board and two collaborators who joined through invitations.
async fn shared_board(env: &TestEnv) -> (User, User, User, Uuid) {
    let owner = adult(env).await;
    let first = adult(env).await;
    let second = adult(env).await;
    make_friends(env, owner.id, first.id).await;
    make_friends(env, owner.id, second.id).await;
    let board = new_board(env, &owner).await;
    join(env, &owner, board, &first).await;
    join(env, &owner, board, &second).await;
    (owner, first, second, board)
}

#[tokio::test]
async fn a_removed_collaborator_loses_their_pins_and_access() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (owner, collaborator, _, board) = shared_board(&env).await;
    let card = deliver_to(&env, &collaborator, &[&owner]).await;
    let pin = Uuid::now_v7();
    apply(&env, &collaborator, board, json!([add_card(pin, card)])).await;
    let version = read_board(&env, &owner, board, "").await["board"]["version"]
        .as_i64()
        .unwrap();

    assert_eq!(
        remove_member(&env, &owner, board, collaborator.id)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let delta = read_board(&env, &owner, board, &format!("?since_version={version}")).await;
    assert_eq!(pin_of(&delta, pin)["kind"], "removed");
    assert_eq!(delta["board"]["version"], version + 1);
    // Their reads and writes are refused like any non-member's.
    let read = call(
        &env,
        "GET",
        &format!("/v1/boards/{board}"),
        &collaborator,
        None,
    )
    .await;
    assert_eq!(read.status(), StatusCode::NOT_FOUND);
    let write = post_ops(&env, &collaborator, board, json!([remove(pin, 2)])).await;
    assert_eq!(write.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        remove_member(&env, &owner, board, collaborator.id)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn collaborators_leave_but_cannot_remove_others_and_the_owner_cannot_leave() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (owner, first, second, board) = shared_board(&env).await;
    // A collaborator removing another collaborator, or the owner: the missing 404.
    let denied = remove_member(&env, &first, board, second.id).await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    let denied = without_request_id(body_bytes(denied).await);
    let missing = remove_member(&env, &first, Uuid::now_v7(), second.id).await;
    assert_eq!(without_request_id(body_bytes(missing).await), denied);
    assert_eq!(
        remove_member(&env, &first, board, owner.id).await.status(),
        StatusCode::NOT_FOUND
    );
    let outsider = adult(&env).await;
    assert_eq!(
        remove_member(&env, &outsider, board, first.id)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_problem(
        remove_member(&env, &owner, board, owner.id).await,
        StatusCode::BAD_REQUEST,
        "VALIDATION_FAILED",
    )
    .await;

    assert_eq!(
        remove_member(&env, &first, board, first.id).await.status(),
        StatusCode::NO_CONTENT
    );
    let read = call(&env, "GET", &format!("/v1/boards/{board}"), &first, None).await;
    assert_eq!(read.status(), StatusCode::NOT_FOUND);
    let members = call(
        &env,
        "GET",
        &format!("/v1/boards/{board}/members"),
        &owner,
        None,
    )
    .await;
    assert_eq!(
        json_body(members).await["items"].as_array().unwrap().len(),
        2
    );
    // The seat is free again: the owner can invite someone else.
    let next = adult(&env).await;
    make_friends(&env, owner.id, next.id).await;
    invited(&env, &owner, board, &next).await;
}

#[tokio::test]
async fn a_block_between_members_ends_their_co_membership() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    // Collaborator blocks collaborator: the blocker leaves.
    let (owner, first, second, board) = shared_board(&env).await;
    block(&env, first.id, second.id).await;
    assert_eq!(
        resolver(&env).resolve(first.id, second.id).await.unwrap(),
        1
    );
    assert_eq!(status_of(&env, &first, board).await, StatusCode::NOT_FOUND);
    assert_eq!(status_of(&env, &second, board).await, StatusCode::OK);
    assert_eq!(
        resolver(&env).resolve(first.id, second.id).await.unwrap(),
        0
    );

    // Owner blocks a collaborator (through the API, which publishes the event): the blocked one
    // is removed, with their pins, and an open invitation between them is cancelled.
    let card = deliver_to(&env, &second, &[&owner]).await;
    let pin = Uuid::now_v7();
    apply(&env, &second, board, json!([add_card(pin, card)])).await;
    let other_board = new_board(&env, &owner).await;
    let open_invitation = invited(&env, &owner, other_board, &second).await;
    let blocked = call(
        &env,
        "POST",
        "/v1/blocks",
        &owner,
        Some(&json!({ "user_id": second.id })),
    )
    .await;
    assert_eq!(blocked.status(), StatusCode::NO_CONTENT);
    let event: (String, serde_json::Value) = sqlx::query_as(
        "SELECT topic, payload FROM platform.outbox_events WHERE topic = 'social.user_blocked'
         AND payload->>'blocker_id' = $1 ORDER BY id DESC LIMIT 1",
    )
    .bind(owner.id.to_string())
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    let job = boards::ResolveBlocksOnEvents
        .job(&platform::outbox::StoredEvent {
            id: 1,
            topic: event.0,
            payload: event.1,
            created_at: env.clock.now(),
        })
        .unwrap();
    assert_eq!(job.kind, boards::RESOLVE_BLOCK_JOB);
    assert_eq!(
        resolver(&env).resolve(owner.id, second.id).await.unwrap(),
        1
    );
    assert_eq!(status_of(&env, &second, board).await, StatusCode::NOT_FOUND);
    assert_eq!(status_of(&env, &owner, board).await, StatusCode::OK);
    let full = read_board(&env, &owner, board, "").await;
    assert!(full["pins"].as_array().unwrap().is_empty());
    let accept = call(
        &env,
        "POST",
        &format!("/v1/board-invitations/{open_invitation}/accept"),
        &second,
        None,
    )
    .await;
    assert_eq!(accept.status(), StatusCode::NOT_FOUND);
}
