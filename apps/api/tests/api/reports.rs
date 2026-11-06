//! T-141: reports (US-26 AC-26.1, SR-31). Visible targets only (uniform 404 otherwise), one
//! report per reporter and target, notes bounded, no automatic action.

use crate::board_ops::add_collaborator;
use crate::boards::new_board;
use crate::cards_read::{deliver_to, delivered_card};
use crate::common::*;
use crate::media::without_request_id;
use crate::public_links::{created_link, token_of};
use crate::social_support::*;
use axum::body::Body;
use http::{Request, Response, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn report(env: &TestEnv, user: &User, body: &Value) -> Response<Body> {
    let request = Request::post("/v1/reports")
        .header("authorization", format!("Bearer {}", user.token))
        .header("content-type", "application/json")
        .header("idempotency-key", Uuid::now_v7().to_string())
        .body(Body::from(body.to_string()))
        .unwrap();
    send(&env.router, request).await
}

pub fn target(kind: &str, id: impl ToString, reason: &str) -> Value {
    json!({ "target": { "type": kind, "id": id.to_string() }, "reason": reason })
}

/// Files a report and returns its id.
pub async fn reported(env: &TestEnv, user: &User, body: &Value) -> String {
    let response = report(env, user, body).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = json_body(response).await;
    assert_eq!(body["report"]["status"], "received");
    body["report"]["id"].as_str().unwrap().to_owned()
}

async fn stored(env: &TestEnv, id: &str) -> (String, String, Uuid, String, i16, Option<String>) {
    sqlx::query_as(
        "SELECT target_type, reason, target_owner_id, status, priority, note
         FROM moderation.reports WHERE id = $1",
    )
    .bind(Uuid::parse_str(id).unwrap())
    .fetch_one(env.identity.pool())
    .await
    .unwrap()
}

async fn missing_body(env: &TestEnv, user: &User) -> String {
    let response = report(env, user, &target("card", Uuid::now_v7(), "spam")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    without_request_id(body_bytes(response).await)
}

async fn assert_not_reportable(env: &TestEnv, user: &User, body: &Value, missing: &str) {
    let response = report(env, user, body).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND, "{body}");
    assert_eq!(without_request_id(body_bytes(response).await), missing);
}

#[tokio::test]
async fn a_recipient_reports_a_card_once() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let delivered = delivered_card(&env).await;
    let mut body = target("card", delivered.card, "harassment");
    body["note"] = json!("  mean caption \n");
    let id = reported(&env, &delivered.friend, &body).await;
    let (kind, reason, owner, status, priority, note) = stored(&env, &id).await;
    assert_eq!(kind, "card");
    assert_eq!(reason, "harassment");
    assert_eq!(owner, delivered.owner.id);
    assert_eq!(status, "open");
    assert_eq!(priority, 50);
    assert_eq!(note.as_deref(), Some("mean caption"));

    // Again (another reason, another key): the first report, nothing new stored.
    let again = reported(
        &env,
        &delivered.friend,
        &target("card", delivered.card, "spam"),
    )
    .await;
    assert_eq!(again, id);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM moderation.reports WHERE target_id = $1")
            .bind(delivered.card)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(count, 1);

    // No automatic action: the owner is still active and the card still visible.
    let status: String = sqlx::query_scalar("SELECT status FROM identity.accounts WHERE id = $1")
        .bind(delivered.owner.id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(status, "active");
}

#[tokio::test]
async fn minor_safety_is_prioritised() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let delivered = delivered_card(&env).await;
    let id = reported(
        &env,
        &delivered.friend,
        &target("card", delivered.card, "minor_safety"),
    )
    .await;
    assert_eq!(stored(&env, &id).await.4, 100);
}

#[tokio::test]
async fn invisible_targets_are_the_missing_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let delivered = delivered_card(&env).await;
    let stranger = adult(&env).await;
    let missing = missing_body(&env, &stranger).await;

    // A card the caller never received, and the owner's own card.
    assert_not_reportable(
        &env,
        &stranger,
        &target("card", delivered.card, "spam"),
        &missing,
    )
    .await;
    assert_not_reportable(
        &env,
        &delivered.owner,
        &target("card", delivered.card, "spam"),
        &missing,
    )
    .await;
    // A malformed id.
    assert_not_reportable(
        &env,
        &stranger,
        &target("card", "not-a-uuid", "spam"),
        &missing,
    )
    .await;

    // A board the caller is not a member of, and one's own board.
    let board = new_board(&env, &delivered.owner).await;
    assert_not_reportable(&env, &stranger, &target("board", board, "hate"), &missing).await;
    assert_not_reportable(
        &env,
        &delivered.owner,
        &target("board", board, "hate"),
        &missing,
    )
    .await;

    // A user who is neither a friend nor blocked, and oneself.
    assert_not_reportable(
        &env,
        &stranger,
        &target("user", delivered.owner.id, "harassment"),
        &missing,
    )
    .await;
    assert_not_reportable(
        &env,
        &stranger,
        &target("user", stranger.id, "spam"),
        &missing,
    )
    .await;

    // An unknown or malformed public-link token.
    assert_not_reportable(
        &env,
        &stranger,
        &target("public_link", "AAAAAAAAAAAAAAAAAAAAAA", "spam"),
        &missing,
    )
    .await;

    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM moderation.reports WHERE reporter_id = ANY($1)")
            .bind(vec![stranger.id, delivered.owner.id])
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn boards_links_and_users_the_caller_can_see_are_reportable() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let member = adult(&env).await;
    let board = new_board(&env, &owner).await;
    add_collaborator(&env, board, &member).await;
    let id = reported(&env, &member, &target("board", board, "violence")).await;
    assert_eq!(stored(&env, &id).await.2, owner.id);

    // A live public link, reported by its token by anyone holding the URL.
    let link = created_link(&env, &owner, board, "7d").await;
    let stranger = adult(&env).await;
    let id = reported(
        &env,
        &stranger,
        &target("public_link", token_of(&link), "sexual"),
    )
    .await;
    let (kind, _, link_owner, _, _, _) = stored(&env, &id).await;
    assert_eq!(kind, "public_link");
    assert_eq!(link_owner, owner.id);
    let link_id: Uuid =
        sqlx::query_scalar("SELECT target_id FROM moderation.reports WHERE id = $1")
            .bind(Uuid::parse_str(&id).unwrap())
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(link_id.to_string(), link["id"].as_str().unwrap());

    // A friend, and someone the caller blocked.
    make_friends(&env, owner.id, stranger.id).await;
    reported(&env, &stranger, &target("user", owner.id, "harassment")).await;
    let pest = adult(&env).await;
    block(&env, stranger.id, pest.id).await;
    reported(&env, &stranger, &target("user", pest.id, "spam")).await;

    // A card on a shared board's owner's feed: delivered cards stay reportable by recipients.
    let card = deliver_to(&env, &owner, &[&stranger]).await;
    reported(&env, &stranger, &target("card", card, "other")).await;
}

#[tokio::test]
async fn bodies_are_validated() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let delivered = delivered_card(&env).await;
    let user = &delivered.friend;
    for body in [
        target("card", delivered.card, "nudity"),
        target("photo", delivered.card, "spam"),
        json!({ "target": { "type": "card", "id": delivered.card }, "reason": "spam", "owner_id": user.id }),
        {
            let mut body = target("card", delivered.card, "spam");
            body["note"] = json!("x".repeat(501));
            body
        },
        {
            let mut body = target("card", delivered.card, "spam");
            body["note"] = json!("bad\u{0}note");
            body
        },
    ] {
        let response = report(&env, user, &body).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
    }
    // The Idempotency-Key is required.
    let request = Request::post("/v1/reports")
        .header("authorization", format!("Bearer {}", user.token))
        .header("content-type", "application/json")
        .body(Body::from(
            target("card", delivered.card, "spam").to_string(),
        ))
        .unwrap();
    assert_eq!(
        send(&env.router, request).await.status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn erasure_keeps_the_report_without_the_reporter() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let delivered = delivered_card(&env).await;
    let mut body = target("card", delivered.card, "hate");
    body["note"] = json!("context");
    let id = reported(&env, &delivered.friend, &body).await;
    let registry = api::privacy::registry(&env.state);
    let export = registry.export(delivered.friend.id).await.unwrap();
    let filed = &export["moderation"].data["reports_filed"];
    assert_eq!(filed[0]["id"], id);
    assert_eq!(filed[0]["note"], "context");
    registry.erase(delivered.friend.id).await.unwrap();
    let (reporter, note): (Option<Uuid>, Option<String>) =
        sqlx::query_as("SELECT reporter_id, note FROM moderation.reports WHERE id = $1")
            .bind(Uuid::parse_str(&id).unwrap())
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!((reporter, note), (None, None));
}
