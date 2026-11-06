//! Rename, duplicate and delete boards (T-112, US-20): owner only, conditional renames, personal
//! duplicates, and deleting a board never deletes cards.

use crate::board_invitations::{invited, join};
use crate::board_ops::{add_card, apply, setup};
use crate::board_read::read_board;
use crate::cards_read::deliver_to;
use crate::common::*;
use crate::social_support::*;
use http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

async fn patch(
    env: &TestEnv,
    user: &User,
    board: Uuid,
    body: Value,
) -> http::Response<axum::body::Body> {
    call(
        env,
        "PATCH",
        &format!("/v1/boards/{board}"),
        user,
        Some(&body),
    )
    .await
}

async fn duplicate(
    env: &TestEnv,
    user: &User,
    board: Uuid,
    copy: Uuid,
) -> http::Response<axum::body::Body> {
    call(
        env,
        "POST",
        &format!("/v1/boards/{board}/duplicate"),
        user,
        Some(&json!({ "id": copy, "title": "Copy" })),
    )
    .await
}

#[tokio::test]
async fn the_owner_renames_with_a_version_check_and_collaborators_cannot() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let renamed = patch(
        &env,
        &s.owner,
        s.board,
        json!({ "base_version": 1, "title": " Trip " }),
    )
    .await;
    assert_eq!(renamed.status(), StatusCode::OK);
    let board = json_body(renamed).await["board"].clone();
    assert_eq!(board["title"], "Trip");
    assert_eq!(board["version"], 2);
    let stale = patch(
        &env,
        &s.owner,
        s.board,
        json!({ "base_version": 1, "title": "Older" }),
    )
    .await;
    assert_problem(stale, StatusCode::CONFLICT, "BOARD_VERSION_CONFLICT").await;
    let reset = patch(
        &env,
        &s.owner,
        s.board,
        json!({ "base_version": 2, "backdrop": null }),
    )
    .await;
    assert_eq!(reset.status(), StatusCode::OK);
    let unknown = patch(
        &env,
        &s.owner,
        s.board,
        json!({ "base_version": 3, "backdrop": { "id": "bg.nope", "version": 1 } }),
    )
    .await;
    assert_problem(
        unknown,
        StatusCode::UNPROCESSABLE_ENTITY,
        "UNKNOWN_REGISTRY_ID",
    )
    .await;
    for body in [
        json!({ "base_version": 3 }),
        json!({ "base_version": 3, "title": "" }),
        json!({ "base_version": 3, "title": "ok", "owner_id": s.friend.id }),
    ] {
        assert_problem(
            patch(&env, &s.owner, s.board, body).await,
            StatusCode::BAD_REQUEST,
            "VALIDATION_FAILED",
        )
        .await;
    }

    join(&env, &s.owner, s.board, &s.friend).await;
    let denied = patch(
        &env,
        &s.friend,
        s.board,
        json!({ "base_version": 3, "title": "Mine" }),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    let denied = call(
        &env,
        "DELETE",
        &format!("/v1/boards/{}", s.board),
        &s.friend,
        None,
    )
    .await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        read_board(&env, &s.owner, s.board, "").await["board"]["title"],
        "Trip"
    );
}

#[tokio::test]
async fn deleting_a_board_keeps_every_card_and_cancels_invitations() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    apply(
        &env,
        &s.owner,
        s.board,
        json!([add_card(Uuid::now_v7(), s.card)]),
    )
    .await;
    let collaborator = adult(&env).await;
    let invitee = adult(&env).await;
    make_friends(&env, s.owner.id, collaborator.id).await;
    make_friends(&env, s.owner.id, invitee.id).await;
    join(&env, &s.owner, s.board, &collaborator).await;
    invited(&env, &s.owner, s.board, &invitee).await;

    let deleted = call(
        &env,
        "DELETE",
        &format!("/v1/boards/{}", s.board),
        &s.owner,
        None,
    )
    .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    for user in [&s.owner, &collaborator] {
        let read = call(&env, "GET", &format!("/v1/boards/{}", s.board), user, None).await;
        assert_eq!(read.status(), StatusCode::NOT_FOUND);
    }
    let invitations = call(&env, "GET", "/v1/me/board-invitations", &invitee, None).await;
    assert!(
        json_body(invitations).await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // The pinned card is untouched for its owner and its recipient.
    for user in [&s.owner, &s.friend] {
        let card = call(&env, "GET", &format!("/v1/cards/{}", s.card), user, None).await;
        assert_eq!(card.status(), StatusCode::OK);
    }
    let again = call(
        &env,
        "DELETE",
        &format!("/v1/boards/{}", s.board),
        &s.owner,
        None,
    )
    .await;
    assert_eq!(again.status(), StatusCode::NOT_FOUND);
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.outbox_events
         WHERE topic = 'boards.board_deleted' AND payload->>'board_id' = $1",
    )
    .bind(s.board.to_string())
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(events, 1);
}

#[tokio::test]
async fn personal_boards_duplicate_with_the_pins_the_owner_may_still_pin() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let received = deliver_to(&env, &s.friend, &[&s.owner]).await;
    apply(
        &env,
        &s.owner,
        s.board,
        json!([
            add_card(Uuid::now_v7(), s.card),
            add_card(Uuid::now_v7(), received)
        ]),
    )
    .await;
    // After a block the received card may no longer be pinned, so the copy leaves it out.
    block(&env, s.friend.id, s.owner.id).await;
    let copy = Uuid::now_v7();
    let created = duplicate(&env, &s.owner, s.board, copy).await;
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_eq!(json_body(created).await["board"]["title"], "Copy");
    let body = read_board(&env, &s.owner, copy, "").await;
    let pins = body["pins"].as_array().unwrap();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0]["card"]["id"], s.card.to_string());
    assert_eq!(
        duplicate(&env, &s.owner, s.board, copy).await.status(),
        StatusCode::OK
    );
    // The source is unchanged.
    assert_eq!(
        read_board(&env, &s.owner, s.board, "").await["pins"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Shared boards cannot be duplicated; collaborators and strangers get the missing 404.
    let collaborator = adult(&env).await;
    make_friends(&env, s.owner.id, collaborator.id).await;
    join(&env, &s.owner, copy, &collaborator).await;
    let shared = duplicate(&env, &s.owner, copy, Uuid::now_v7()).await;
    assert_problem(shared, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    let denied = duplicate(&env, &collaborator, copy, Uuid::now_v7()).await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
}
