//! T-75: create and send a card. Media must be the caller's and of the right kind; recipients are
//! intersected with current friends silently; the body is an allow-list; replays return the
//! original 202; 100 sends per day.

use crate::common::*;
use crate::media_gate::fake_gates;
use crate::media_process::{real_png, uploaded_id};
use crate::social_support::*;
use axum::body::Body;
use http::{Request, Response, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

pub fn style() -> Value {
    json!({ "cut": { "id": "cut.diecut", "version": 1, "params": { "borderWidthPct": 2.5 } } })
}

/// A processed (clean) render and a matching mask of `user`.
pub async fn clean_media(env: &TestEnv, user: &User) -> (Uuid, Uuid) {
    let render = uploaded_id(env, user, "card_render", real_png(120, 90)).await;
    let mask = uploaded_id(env, user, "mask", real_png(120, 90)).await;
    for id in [render, mask] {
        env.state
            .media
            .process_one(id, &fake_gates())
            .await
            .unwrap();
    }
    (render, mask)
}

pub fn card_body(render: Uuid, mask: Option<Uuid>, audience: Value) -> Value {
    json!({
        "client_card_id": Uuid::now_v7(),
        "render_media_id": render,
        "mask_media_id": mask,
        "style": style(),
        "caption": "zoomies",
        "audience": audience,
    })
}

/// `POST /v1/cards` with `Idempotency-Key` = `client_card_id`.
pub async fn post_card(env: &TestEnv, user: &User, body: &Value) -> Response<Body> {
    let key = body["client_card_id"].as_str().unwrap_or("").to_owned();
    let request = Request::post("/v1/cards")
        .header("authorization", format!("Bearer {}", user.token))
        .header("content-type", "application/json")
        .header("idempotency-key", key)
        .body(Body::from(body.to_string()))
        .unwrap();
    send(&env.router, request).await
}

/// Sends a card from `sender` to `recipients` (subset) and returns its id.
pub async fn sent_card(env: &TestEnv, sender: &User, recipients: &[&User]) -> Uuid {
    let (render, mask) = clean_media(env, sender).await;
    let ids: Vec<String> = recipients.iter().map(|u| u.id.to_string()).collect();
    let body = card_body(
        render,
        Some(mask),
        json!({ "mode": "subset", "recipient_ids": ids }),
    );
    let response = post_card(env, sender, &body).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    Uuid::parse_str(json_body(response).await["card"]["id"].as_str().unwrap()).unwrap()
}

async fn recipient_ids(env: &TestEnv, card: &str) -> Vec<Uuid> {
    sqlx::query_scalar(
        "SELECT recipient_id FROM cards.card_recipients WHERE card_id = $1::uuid ORDER BY recipient_id",
    )
    .bind(card)
    .fetch_all(env.identity.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn non_friends_are_dropped_silently() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sender = adult(&env).await;
    let friend = adult(&env).await;
    let stranger = adult(&env).await;
    make_friends(&env, sender.id, friend.id).await;
    let (render, mask) = clean_media(&env, &sender).await;
    let body = card_body(
        render,
        Some(mask),
        json!({ "mode": "subset", "recipient_ids": [friend.id, stranger.id] }),
    );
    let response = post_card(&env, &sender, &body).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let json = json_body(response).await;
    assert_eq!(json["card"]["status"], "pending_scan");
    assert_eq!(json["card"]["recipient_count"], 1);
    let text = json.to_string();
    assert!(!text.contains(&stranger.id.to_string()));
    assert!(!text.contains(&friend.id.to_string()));
    let card = json["card"]["id"].as_str().unwrap();
    assert_eq!(recipient_ids(&env, card).await, vec![friend.id]);
    // Not delivered yet: nobody sees it before the gates (T-76).
    let delivered: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cards.card_recipients WHERE card_id = $1::uuid AND delivered_at IS NOT NULL",
    )
    .bind(card)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(delivered, 0);
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.outbox_events WHERE topic = 'cards.submitted' AND payload->>'card_id' = $1",
    )
    .bind(card)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(events, 1);
}

#[tokio::test]
async fn audience_all_means_current_friends_only() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sender = adult(&env).await;
    let a = adult(&env).await;
    let b = adult(&env).await;
    let blocked = adult(&env).await;
    make_friends(&env, sender.id, a.id).await;
    make_friends(&env, sender.id, b.id).await;
    // Blocking ends a friendship; a block row alone never makes a recipient.
    block(&env, blocked.id, sender.id).await;
    let (render, _) = clean_media(&env, &sender).await;
    let response = post_card(
        &env,
        &sender,
        &card_body(render, None, json!({ "mode": "all" })),
    )
    .await;
    let json = json_body(response).await;
    let mut expected = vec![a.id, b.id];
    expected.sort();
    assert_eq!(
        recipient_ids(&env, json["card"]["id"].as_str().unwrap()).await,
        expected
    );
}

#[tokio::test]
async fn someone_elses_media_is_404_and_wrong_kinds_too() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sender = adult(&env).await;
    let other = adult(&env).await;
    let (theirs, their_mask) = clean_media(&env, &other).await;
    let (mine, my_mask) = clean_media(&env, &sender).await;
    for body in [
        card_body(theirs, None, json!({ "mode": "all" })),
        card_body(mine, Some(their_mask), json!({ "mode": "all" })),
        // A mask used as the render, a render used as the mask.
        card_body(my_mask, None, json!({ "mode": "all" })),
        card_body(mine, Some(mine), json!({ "mode": "all" })),
        card_body(Uuid::now_v7(), None, json!({ "mode": "all" })),
    ] {
        let response = post_card(&env, &sender, &body).await;
        assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    }
}

#[tokio::test]
async fn mass_assignment_and_bad_bodies_are_400() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sender = adult(&env).await;
    let (render, mask) = clean_media(&env, &sender).await;
    let mut with_owner = card_body(render, None, json!({ "mode": "all" }));
    with_owner["owner_id"] = json!(Uuid::now_v7());
    let mut with_status = card_body(render, None, json!({ "mode": "all" }));
    with_status["status"] = json!("active");
    let mut long_caption = card_body(render, None, json!({ "mode": "all" }));
    long_caption["caption"] = json!("x".repeat(81));
    let too_many: Vec<Uuid> = (0..21).map(|_| Uuid::now_v7()).collect();
    let dup = Uuid::now_v7();
    for body in [
        with_owner,
        with_status,
        long_caption,
        card_body(
            render,
            None,
            json!({ "mode": "subset", "recipient_ids": too_many }),
        ),
        card_body(
            render,
            None,
            json!({ "mode": "subset", "recipient_ids": [dup, dup] }),
        ),
        card_body(render, None, json!({ "mode": "everyone" })),
    ] {
        let response = post_card(&env, &sender, &body).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    // A mask whose size differs from the render.
    let small_mask = uploaded_id(&env, &sender, "mask", real_png(60, 45)).await;
    let response = post_card(
        &env,
        &sender,
        &card_body(render, Some(small_mask), json!({ "mode": "all" })),
    )
    .await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    // Key must equal client_card_id.
    let body = card_body(render, Some(mask), json!({ "mode": "all" }));
    let request = Request::post("/v1/cards")
        .header("authorization", format!("Bearer {}", sender.token))
        .header("content-type", "application/json")
        .header("idempotency-key", Uuid::now_v7().to_string())
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = send(&env.router, request).await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    // Unknown registry ids.
    let mut bad_style = card_body(render, None, json!({ "mode": "all" }));
    bad_style["style"] = json!({ "effects": [{ "id": "fx.unknown", "version": 1 }] });
    let response = post_card(&env, &sender, &bad_style).await;
    assert_problem(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "UNKNOWN_REGISTRY_ID",
    )
    .await;
}

#[tokio::test]
async fn replay_returns_the_original_202() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sender = adult(&env).await;
    let friend = adult(&env).await;
    make_friends(&env, sender.id, friend.id).await;
    let (render, mask) = clean_media(&env, &sender).await;
    let body = card_body(render, Some(mask), json!({ "mode": "all" }));
    let first = post_card(&env, &sender, &body).await;
    assert_eq!(first.status(), StatusCode::ACCEPTED);
    let first = json_body(first).await;
    let again = post_card(&env, &sender, &body).await;
    assert_eq!(again.status(), StatusCode::ACCEPTED);
    assert_eq!(again.headers()["idempotent-replayed"], "true");
    assert_eq!(json_body(again).await, first);
    let cards: i64 = sqlx::query_scalar("SELECT count(*) FROM cards.cards WHERE owner_id = $1")
        .bind(sender.id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(cards, 1);
}

#[tokio::test]
async fn the_101st_send_of_the_day_is_429() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sender = established(&env, adult(&env).await).await;
    let (render, _) = clean_media(&env, &sender).await;
    for n in 0..100 {
        let response = post_card(
            &env,
            &sender,
            &card_body(render, None, json!({ "mode": "all" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED, "send {n}");
    }
    let response = post_card(
        &env,
        &sender,
        &card_body(render, None, json!({ "mode": "all" })),
    )
    .await;
    assert_problem(response, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED").await;
}
