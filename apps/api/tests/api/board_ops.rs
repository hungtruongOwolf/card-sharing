//! Pin op batches (T-99, AC-18.2, AC-19.2, AC-19.3): per-op results, versions, replays, limits
//! and pin ownership.

use crate::boards::new_board;
use crate::cards_read::deliver_to;
use crate::common::*;
use crate::media::without_request_id;
use crate::social_support::*;
use http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn post_ops(
    env: &TestEnv,
    user: &User,
    board: Uuid,
    ops: Value,
) -> http::Response<axum::body::Body> {
    call(
        env,
        "POST",
        &format!("/v1/boards/{board}/ops"),
        user,
        Some(&json!({ "ops": ops })),
    )
    .await
}

/// Applies `ops` and returns the 200 body.
pub async fn apply(env: &TestEnv, user: &User, board: Uuid, ops: Value) -> Value {
    let response = post_ops(env, user, board, ops).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

pub fn add_card(pin: Uuid, card: Uuid) -> Value {
    json!({ "type": "pin.add", "op_id": Uuid::now_v7(), "pin": {
        "id": pin, "card_id": card, "x": 0.4, "y": 0.2, "scale": 1, "rotation": -8, "z_key": "a0V" } })
}

pub fn update(pin: Uuid, base_version: i64, set: Value) -> Value {
    json!({ "type": "pin.update", "op_id": Uuid::now_v7(), "pin_id": pin, "base_version": base_version, "set": set })
}

pub fn remove(pin: Uuid, base_version: i64) -> Value {
    json!({ "type": "pin.remove", "op_id": Uuid::now_v7(), "pin_id": pin, "base_version": base_version })
}

/// Makes `user` a collaborator of `board` directly (the invitation flow has its own tests).
pub async fn add_collaborator(env: &TestEnv, board: Uuid, user: &User) {
    sqlx::query(
        "INSERT INTO boards.board_members (board_id, user_id, role, created_at)
         VALUES ($1, $2, 'collaborator', now())",
    )
    .bind(board)
    .bind(user.id)
    .execute(env.identity.pool())
    .await
    .unwrap();
}

/// A board owned by a new adult with one of their own active cards, delivered to a friend.
pub struct Setup {
    pub owner: User,
    pub friend: User,
    pub board: Uuid,
    pub card: Uuid,
}

pub async fn setup(env: &TestEnv) -> Setup {
    let owner = adult(env).await;
    let friend = adult(env).await;
    make_friends(env, owner.id, friend.id).await;
    let card = deliver_to(env, &owner, &[&friend]).await;
    let board = new_board(env, &owner).await;
    Setup {
        owner,
        friend,
        board,
        card,
    }
}

async fn board_version(env: &TestEnv, user: &User, board: Uuid) -> i64 {
    let response = call(env, "GET", &format!("/v1/boards/{board}"), user, None).await;
    json_body(response).await["board"]["version"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn ops_apply_with_versions_conflicts_and_replays() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let pin = Uuid::now_v7();
    let add = add_card(pin, s.card);
    let body = apply(&env, &s.owner, s.board, json!([add.clone()])).await;
    assert_eq!(body["board_version"], 2);
    let result = &body["results"][0];
    assert_eq!(result["status"], "applied");
    assert_eq!(result["pin"]["version"], 1);
    assert_eq!(result["pin"]["card_id"], s.card.to_string());
    assert_eq!(result["pin"]["mine"], true);
    assert!((result["pin"]["rotation"].as_f64().unwrap() + 8.0).abs() < 1e-9);
    assert_eq!(board_version(&env, &s.owner, s.board).await, 2);

    // Replaying the op, or re-adding the same pin id, changes nothing.
    let replay = apply(&env, &s.owner, s.board, json!([add])).await;
    assert_eq!(replay["board_version"], 2);
    assert_eq!(replay["results"][0]["status"], "applied");
    let readd = apply(&env, &s.owner, s.board, json!([add_card(pin, s.card)])).await;
    assert_eq!(readd["board_version"], 2);
    assert_eq!(readd["results"][0]["pin"]["version"], 1);

    // AC-19.3: two clients edit the same pin from version 1; the second gets a conflict with the
    // current pin, and the other op of its batch still applies.
    let first = apply(
        &env,
        &s.owner,
        s.board,
        json!([update(pin, 1, json!({ "x": 0.9 }))]),
    )
    .await;
    assert_eq!(first["results"][0]["pin"]["version"], 2);
    let other = Uuid::now_v7();
    let second = apply(
        &env,
        &s.owner,
        s.board,
        json!([
            update(pin, 1, json!({ "rotation": 4 })),
            add_card(other, s.card)
        ]),
    )
    .await;
    assert_eq!(second["results"][0]["status"], "conflict");
    assert_eq!(second["results"][0]["current"]["version"], 2);
    assert_eq!(second["results"][0]["current"]["x"], 0.9);
    assert_eq!(second["results"][1]["status"], "applied");
    assert_eq!(second["board_version"], 4);

    let removed = apply(&env, &s.owner, s.board, json!([remove(pin, 2)])).await;
    assert_eq!(removed["results"][0]["pin"]["removed"], true);
    let gone = apply(
        &env,
        &s.owner,
        s.board,
        json!([update(pin, 3, json!({ "x": 0.1 }))]),
    )
    .await;
    assert_eq!(gone["results"][0]["code"], "NOT_FOUND");
    assert_eq!(gone["board_version"], 5);

    let actors: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT actor_id, seq FROM boards.board_ops WHERE board_id = $1 ORDER BY seq",
    )
    .bind(s.board)
    .fetch_all(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(
        actors.iter().map(|a| a.1).collect::<Vec<_>>(),
        vec![2, 3, 4, 5]
    );
    assert!(actors.iter().all(|a| a.0 == s.owner.id));
}

#[tokio::test]
async fn cards_the_pinner_does_not_own_and_foreign_ids_are_rejected() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let stranger = adult(&env).await;
    let strangers_friend = adult(&env).await;
    make_friends(&env, stranger.id, strangers_friend.id).await;
    let foreign_card = deliver_to(&env, &stranger, &[&strangers_friend]).await;
    let body = apply(
        &env,
        &s.owner,
        s.board,
        json!([
            add_card(Uuid::now_v7(), foreign_card),
            add_card(Uuid::now_v7(), Uuid::now_v7())
        ]),
    )
    .await;
    assert_eq!(body["results"][0]["code"], "NOT_FOUND");
    assert_eq!(body["results"][1]["code"], "NOT_FOUND");
    assert_eq!(body["board_version"], 1);

    // Another account's op id or pin id is just an unknown id.
    let strangers_board = new_board(&env, &stranger).await;
    let strangers_pin = Uuid::now_v7();
    let strangers_card = deliver_to(&env, &stranger, &[&strangers_friend]).await;
    let add = add_card(strangers_pin, strangers_card);
    apply(&env, &stranger, strangers_board, json!([add.clone()])).await;
    let replayed = apply(&env, &s.owner, s.board, json!([add])).await;
    assert_eq!(replayed["results"][0]["code"], "NOT_FOUND");
    let foreign_pin = apply(
        &env,
        &s.owner,
        s.board,
        json!([
            update(strangers_pin, 1, json!({ "x": 0.1 })),
            remove(strangers_pin, 1)
        ]),
    )
    .await;
    assert_eq!(foreign_pin["results"][0]["code"], "NOT_FOUND");
    assert_eq!(foreign_pin["results"][1]["code"], "NOT_FOUND");

    // A non-member's batch is the missing 404, and nothing changes on the board.
    let denied = post_ops(
        &env,
        &s.owner,
        strangers_board,
        json!([remove(strangers_pin, 1)]),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    let denied = without_request_id(body_bytes(denied).await);
    let missing = post_ops(
        &env,
        &s.owner,
        Uuid::now_v7(),
        json!([remove(strangers_pin, 1)]),
    )
    .await;
    assert_eq!(without_request_id(body_bytes(missing).await), denied);
    assert_eq!(board_version(&env, &stranger, strangers_board).await, 2);
}

#[tokio::test]
async fn collaborators_change_only_their_own_pins() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let collaborator = s.friend.clone();
    add_collaborator(&env, s.board, &collaborator).await;
    let owners_pin = Uuid::now_v7();
    apply(
        &env,
        &s.owner,
        s.board,
        json!([add_card(owners_pin, s.card)]),
    )
    .await;

    let other = adult(&env).await;
    make_friends(&env, collaborator.id, other.id).await;
    let own_card = deliver_to(&env, &collaborator, &[&other]).await;
    let collaborators_pin = Uuid::now_v7();
    let body = apply(
        &env,
        &collaborator,
        s.board,
        json!([
            add_card(collaborators_pin, own_card),
            // A stale base version does not reveal the owner's pin: ownership is checked first.
            update(owners_pin, 99, json!({ "x": 0.1 })),
            remove(owners_pin, 1),
        ]),
    )
    .await;
    assert_eq!(body["results"][0]["status"], "applied");
    assert_eq!(body["results"][1]["code"], "PIN_NOT_YOURS");
    assert_eq!(body["results"][2]["code"], "PIN_NOT_YOURS");

    // The owner may move and remove any pin, but never learns the card id of someone else's pin.
    let moved = apply(
        &env,
        &s.owner,
        s.board,
        json!([update(collaborators_pin, 1, json!({ "y": 0.7 }))]),
    )
    .await;
    let pin = &moved["results"][0]["pin"];
    assert_eq!(moved["results"][0]["status"], "applied");
    assert_eq!(pin["mine"], false);
    assert!(pin.get("card_id").is_none());
    let removed = apply(
        &env,
        &s.owner,
        s.board,
        json!([remove(collaborators_pin, 2)]),
    )
    .await;
    assert_eq!(removed["results"][0]["status"], "applied");
}

#[tokio::test]
async fn the_hundred_and_first_pin_is_rejected() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    for _ in 0..2 {
        let ops: Vec<Value> = (0..50).map(|_| add_card(Uuid::now_v7(), s.card)).collect();
        let body = apply(&env, &s.owner, s.board, json!(ops)).await;
        assert!(
            body["results"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["status"] == "applied")
        );
    }
    let body = apply(
        &env,
        &s.owner,
        s.board,
        json!([add_card(Uuid::now_v7(), s.card)]),
    )
    .await;
    assert_eq!(body["results"][0]["code"], "PIN_LIMIT_REACHED");
    assert_eq!(body["board_version"], 101);
}

#[tokio::test]
async fn malformed_batches_are_refused_as_a_whole() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let op_id = Uuid::now_v7();
    let pin_with = |fields: Value| {
        let mut pin = json!({ "id": Uuid::now_v7(), "card_id": s.card, "x": 0.5, "y": 0.5,
                              "scale": 1, "rotation": 0, "z_key": "a" });
        for (k, v) in fields.as_object().unwrap() {
            pin[k] = v.clone();
        }
        json!({ "type": "pin.add", "op_id": Uuid::now_v7(), "pin": pin })
    };
    let too_many: Vec<Value> = (0..51).map(|_| add_card(Uuid::now_v7(), s.card)).collect();
    for ops in [
        json!([]),
        json!(too_many),
        json!([pin_with(json!({ "scale": 1e308 }))]),
        json!([pin_with(json!({ "x": 1.5 }))]),
        json!([pin_with(json!({ "rotation": 720 }))]),
        json!([pin_with(json!({ "z_key": "a b" }))]),
        json!([pin_with(
            json!({ "id": "0192f0c1-7c1e-4a3b-9d2e-123456789abc" })
        )]),
        json!([pin_with(
            json!({ "decoration": { "id": "deco.washi", "version": 1 } })
        )]),
        json!([pin_with(json!({ "created_by": s.friend.id }))]),
        json!([update(Uuid::now_v7(), 1, json!({}))]),
        json!([update(Uuid::now_v7(), 1, json!({ "scale": 0 }))]),
        json!([
            { "type": "pin.remove", "op_id": op_id, "pin_id": Uuid::now_v7(), "base_version": 1 },
            { "type": "pin.remove", "op_id": op_id, "pin_id": Uuid::now_v7(), "base_version": 1 },
        ]),
    ] {
        let response = post_ops(&env, &s.owner, s.board, ops.clone()).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    let unknown_decoration = json!({ "type": "pin.add", "op_id": Uuid::now_v7(), "pin": {
        "id": Uuid::now_v7(), "decoration": { "id": "deco.unknown", "version": 1 },
        "x": 0.5, "y": 0.5, "scale": 1, "rotation": 0, "z_key": "a" } });
    let response = post_ops(&env, &s.owner, s.board, json!([unknown_decoration])).await;
    assert_problem(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "UNKNOWN_REGISTRY_ID",
    )
    .await;
    assert_eq!(board_version(&env, &s.owner, s.board).await, 1);
}
