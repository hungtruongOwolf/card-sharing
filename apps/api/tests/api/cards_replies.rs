//! ADR-0010: a reply to a card is a card. It goes to the original sender only, and only from
//! someone who may read the original right now (the card-read predicate); anything else is the
//! same 404 as a missing card, and any other audience is 400. The reply carries `reply_to`
//! resolved per viewer, and the original sender lists replies to their card. Every assertion is
//! about users and cards this test created (the database is shared with concurrent tests).

use crate::cards_read::{assert_hidden, deliver_to};
use crate::cards_send::{card_body, clean_media, post_card};
use crate::common::*;
use crate::media::without_request_id;
use crate::social_support::*;
use http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

/// A reply body from `user` to `original` (`audience` omitted when `None`).
async fn reply_body(env: &TestEnv, user: &User, original: Uuid, audience: Option<Value>) -> Value {
    let (render, mask) = clean_media(env, user).await;
    let mut body = card_body(render, Some(mask), json!(null));
    body["reply_to_card_id"] = json!(original);
    match audience {
        Some(audience) => body["audience"] = audience,
        None => {
            body.as_object_mut().unwrap().remove("audience");
        }
    }
    body
}

/// Sends a reply and delivers it (both gates pass); returns its id.
async fn delivered_reply(env: &TestEnv, user: &User, original: Uuid) -> Uuid {
    let body = reply_body(env, user, original, None).await;
    let response = post_card(env, user, &body).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let json = json_body(response).await;
    assert_eq!(json["card"]["recipient_count"], 1);
    let id = Uuid::parse_str(json["card"]["id"].as_str().unwrap()).unwrap();
    env.state
        .cards
        .evaluate(&env.state.media, id)
        .await
        .unwrap();
    id
}

async fn card(env: &TestEnv, user: &User, id: Uuid) -> Value {
    let response = call(env, "GET", &format!("/v1/cards/{id}"), user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["card"].clone()
}

async fn replies(env: &TestEnv, user: &User, id: Uuid) -> http::Response<axum::body::Body> {
    call(env, "GET", &format!("/v1/cards/{id}/replies"), user, None).await
}

#[tokio::test]
async fn a_reply_reaches_the_original_sender_only_with_its_reference() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let carol = adult(&env).await;
    make_friends(&env, alice.id, bob.id).await;
    make_friends(&env, alice.id, carol.id).await;
    make_friends(&env, bob.id, carol.id).await;
    let original = deliver_to(&env, &alice, &[&bob, &carol]).await;

    let reply = delivered_reply(&env, &bob, original).await;
    let recipients: Vec<Uuid> = sqlx::query_scalar(
        "SELECT recipient_id FROM cards.card_recipients WHERE card_id = $1 AND delivered_at IS NOT NULL",
    )
    .bind(reply)
    .fetch_all(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(recipients, vec![alice.id]);

    // Alice (original sender) and Bob (replier) both see the reference; Carol never sees the reply.
    for viewer in [&alice, &bob] {
        let view = card(&env, viewer, reply).await;
        assert_eq!(view["reply_to"]["card_id"], json!(original), "{view}");
    }
    assert_hidden(&env, &carol, reply).await;
    // The original itself is not a reply.
    assert!(card(&env, &alice, original).await["reply_to"].is_null());

    // Alice's feed shows it like a card, with the reference.
    let feed = json_body(call(&env, "GET", "/v1/feed", &alice, None).await).await;
    let item = feed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == json!(reply))
        .cloned()
        .unwrap();
    assert_eq!(item["reply_to"]["card_id"], json!(original));

    // Alice lists the replies to her card; anyone else gets the missing-card 404.
    let response = replies(&env, &alice, original).await;
    assert_eq!(response.status(), StatusCode::OK);
    let items = json_body(response).await["items"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], json!(reply));
    assert_eq!(items[0]["sender"]["id"], json!(bob.id));
    let missing = without_request_id(body_bytes(replies(&env, &bob, Uuid::now_v7()).await).await);
    for other in [&bob, &carol] {
        let denied = replies(&env, other, original).await;
        assert_eq!(denied.status(), StatusCode::NOT_FOUND);
        assert_eq!(without_request_id(body_bytes(denied).await), missing);
    }
}

#[tokio::test]
async fn replying_to_a_card_i_cannot_read_is_the_missing_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let stranger = adult(&env).await;
    make_friends(&env, alice.id, bob.id).await;
    make_friends(&env, alice.id, stranger.id).await;
    // Delivered to Bob only: the stranger is Alice's friend but not a recipient.
    let original = deliver_to(&env, &alice, &[&bob]).await;

    let body = reply_body(&env, &stranger, Uuid::now_v7(), None).await;
    let response = post_card(&env, &stranger, &body).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let missing = without_request_id(body_bytes(response).await);

    let body = reply_body(&env, &stranger, original, None).await;
    let response = post_card(&env, &stranger, &body).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(without_request_id(body_bytes(response).await), missing);

    let mut malformed = reply_body(&env, &bob, original, None).await;
    malformed["reply_to_card_id"] = json!("not-a-uuid");
    let response = post_card(&env, &bob, &malformed).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // A block ends Bob's access to the original: his reply is then the same 404.
    block(&env, alice.id, bob.id).await;
    let body = reply_body(&env, &bob, original, None).await;
    let response = post_card(&env, &bob, &body).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(without_request_id(body_bytes(response).await), missing);
}

#[tokio::test]
async fn a_reply_goes_to_the_original_sender_and_nobody_else() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let carol = adult(&env).await;
    make_friends(&env, alice.id, bob.id).await;
    make_friends(&env, bob.id, carol.id).await;
    let original = deliver_to(&env, &alice, &[&bob]).await;

    for audience in [
        json!({ "mode": "all" }),
        json!({ "mode": "subset", "recipient_ids": [carol.id] }),
        json!({ "mode": "subset", "recipient_ids": [alice.id, carol.id] }),
    ] {
        let body = reply_body(&env, &bob, original, Some(audience)).await;
        let response = post_card(&env, &bob, &body).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    // Exactly the sender is accepted.
    let body = reply_body(
        &env,
        &bob,
        original,
        Some(json!({ "mode": "subset", "recipient_ids": [alice.id] })),
    )
    .await;
    assert_eq!(
        post_card(&env, &bob, &body).await.status(),
        StatusCode::ACCEPTED
    );
    // My own card: 400 (I know it exists).
    let body = reply_body(&env, &alice, original, None).await;
    let response = post_card(&env, &alice, &body).await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    // Without a reply, the audience stays required.
    let (render, _) = clean_media(&env, &bob).await;
    let mut body = card_body(render, None, json!(null));
    body.as_object_mut().unwrap().remove("audience");
    let response = post_card(&env, &bob, &body).await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
}

#[tokio::test]
async fn the_reference_disappears_when_the_original_is_no_longer_readable() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    make_friends(&env, alice.id, bob.id).await;
    let original = deliver_to(&env, &alice, &[&bob]).await;
    let reply = delivered_reply(&env, &bob, original).await;

    let response = call(
        &env,
        "DELETE",
        &format!("/v1/cards/{original}"),
        &alice,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    // The reply stays (it is Bob's card) but quotes nothing, for both of them.
    for viewer in [&alice, &bob] {
        let view = card(&env, viewer, reply).await;
        assert!(view["reply_to"].is_null(), "{view}");
    }
    // A deleted original has no reply list.
    assert_eq!(
        replies(&env, &alice, original).await.status(),
        StatusCode::NOT_FOUND
    );
}
