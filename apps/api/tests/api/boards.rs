//! Boards: create, list and read (T-98, US-18 AC-18.2). Board ids are client-generated UUIDv7s
//! scoped to their owner.

use crate::common::*;
use crate::media::without_request_id;
use crate::social_support::*;
use http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn create_board(
    env: &TestEnv,
    user: &User,
    id: Uuid,
    title: &str,
) -> http::Response<axum::body::Body> {
    call(
        env,
        "POST",
        "/v1/boards",
        user,
        Some(&json!({ "id": id, "title": title })),
    )
    .await
}

/// A new board owned by `user`.
pub async fn new_board(env: &TestEnv, user: &User) -> Uuid {
    let id = Uuid::now_v7();
    let response = create_board(env, user, id, "Summer").await;
    assert_eq!(response.status(), StatusCode::CREATED);
    id
}

pub async fn list(env: &TestEnv, user: &User) -> Value {
    let response = call(env, "GET", "/v1/boards", user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

fn ids(list: &Value) -> Vec<String> {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_str().unwrap().to_owned())
        .collect()
}

/// Asserts a 404 that is byte-identical (request id aside) to a missing board.
pub async fn assert_board_hidden(env: &TestEnv, user: &User, board: Uuid) {
    let denied = call(env, "GET", &format!("/v1/boards/{board}"), user, None).await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    let denied = without_request_id(body_bytes(denied).await);
    let missing = call(
        env,
        "GET",
        &format!("/v1/boards/{}", Uuid::now_v7()),
        user,
        None,
    )
    .await;
    assert_eq!(without_request_id(body_bytes(missing).await), denied);
}

#[tokio::test]
async fn owner_creates_lists_and_reads_a_board() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let id = Uuid::now_v7();
    let response = create_board(&env, &owner, id, "  Summer\u{200B} trip ").await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let board = json_body(response).await["board"].clone();
    assert_eq!(board["id"], id.to_string());
    assert_eq!(board["title"], "Summer trip");
    assert_eq!(board["role"], "owner");
    assert_eq!(board["backdrop"], Value::Null);
    assert_eq!(board["version"], 1);

    assert_eq!(ids(&list(&env, &owner).await), vec![id.to_string()]);
    let read = call(&env, "GET", &format!("/v1/boards/{id}"), &owner, None).await;
    assert_eq!(read.status(), StatusCode::OK);
    assert_eq!(json_body(read).await["board"]["title"], "Summer trip");

    // A retry with the same id returns the same board, unchanged.
    let retry = create_board(&env, &owner, id, "Another title").await;
    assert_eq!(retry.status(), StatusCode::OK);
    assert_eq!(json_body(retry).await["board"]["title"], "Summer trip");
    assert_eq!(
        list(&env, &owner).await["items"].as_array().unwrap().len(),
        1
    );
}

#[tokio::test]
async fn another_owners_id_and_non_members_get_the_missing_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let stranger = adult(&env).await;
    let friend = adult(&env).await;
    make_friends(&env, owner.id, friend.id).await;
    let board = new_board(&env, &owner).await;

    // Colliding with someone else's id never reveals or touches their board.
    let collision = create_board(&env, &stranger, board, "Mine now").await;
    assert_eq!(collision.status(), StatusCode::NOT_FOUND);
    let collision = without_request_id(body_bytes(collision).await);
    let missing = call(
        &env,
        "GET",
        &format!("/v1/boards/{}", Uuid::now_v7()),
        &stranger,
        None,
    )
    .await;
    assert_eq!(without_request_id(body_bytes(missing).await), collision);
    assert!(ids(&list(&env, &stranger).await).is_empty());

    // Friendship alone gives no access: membership requires an accepted invitation.
    for user in [&stranger, &friend] {
        assert_board_hidden(&env, user, board).await;
        assert!(ids(&list(&env, user).await).is_empty());
    }
    let malformed = call(&env, "GET", "/v1/boards/not-a-uuid", &stranger, None).await;
    assert_eq!(malformed.status(), StatusCode::NOT_FOUND);
    let owners_view = call(&env, "GET", &format!("/v1/boards/{board}"), &owner, None).await;
    assert_eq!(json_body(owners_view).await["board"]["title"], "Summer");
}

#[tokio::test]
async fn the_twenty_first_board_is_refused() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    for _ in 0..20 {
        new_board(&env, &owner).await;
    }
    let refused = create_board(&env, &owner, Uuid::now_v7(), "One too many").await;
    assert_problem(refused, StatusCode::CONFLICT, "BOARD_LIMIT_REACHED").await;
    assert_eq!(
        list(&env, &owner).await["items"].as_array().unwrap().len(),
        20
    );
    // Another account is not affected by this owner's cap.
    let other = adult(&env).await;
    new_board(&env, &other).await;
}

#[tokio::test]
async fn invalid_ids_titles_backdrops_and_fields_are_refused() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let post = |body: Value| {
        let env = &env;
        let owner = &owner;
        async move { call(env, "POST", "/v1/boards", owner, Some(&body)).await }
    };
    for body in [
        json!({ "id": "0192f0c1-7c1e-4a3b-9d2e-123456789abc", "title": "v4 id" }),
        json!({ "id": "not-a-uuid", "title": "bad id" }),
        json!({ "id": Uuid::now_v7(), "title": "" }),
        json!({ "id": Uuid::now_v7(), "title": " \u{200B} " }),
        json!({ "id": Uuid::now_v7(), "title": "x".repeat(61) }),
        json!({ "id": Uuid::now_v7(), "title": "ok", "owner_id": owner.id }),
        json!({ "id": Uuid::now_v7(), "title": "ok", "version": 9 }),
        json!({ "title": "no id" }),
    ] {
        assert_problem(
            post(body).await,
            StatusCode::BAD_REQUEST,
            "VALIDATION_FAILED",
        )
        .await;
    }
    for backdrop in [
        json!({ "id": "bg.unknown", "version": 1 }),
        json!({ "id": "cut.diecut", "version": 1 }),
        json!("cork"),
    ] {
        let body = json!({ "id": Uuid::now_v7(), "title": "ok", "backdrop": backdrop });
        assert_problem(
            post(body).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNKNOWN_REGISTRY_ID",
        )
        .await;
    }
    let explicit_default = json!({ "id": Uuid::now_v7(), "title": "ok", "backdrop": null });
    assert_eq!(post(explicit_default).await.status(), StatusCode::CREATED);
    assert_eq!(
        list(&env, &owner).await["items"].as_array().unwrap().len(),
        1
    );
}
