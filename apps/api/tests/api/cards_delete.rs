//! T-82: the owner deletes a sent card; it is hidden everywhere on the next read, its media are
//! purged (old signed URLs stop resolving), and nobody else can delete it.

use crate::cards_read::{assert_hidden, delivered_card, read};
use crate::common::*;
use crate::media::without_request_id;
use crate::social_support::*;
use cards::{CardStatus, PurgeOnDelete};
use http::StatusCode;
use media::Bucket;
use platform::outbox::{StoredEvent, Subscriber};
use uuid::Uuid;

async fn delete(env: &TestEnv, user: &User, card: Uuid) -> http::Response<axum::body::Body> {
    call(env, "DELETE", &format!("/v1/cards/{card}"), user, None).await
}

#[tokio::test]
async fn deleting_hides_the_card_everywhere_and_purges_its_media() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let d = delivered_card(&env).await;
    // The recipient holds a signed URL from before the delete.
    let view = json_body(read(&env, &d.friend, d.card).await).await;
    let old_url = view["card"]["media"]["render_url"]
        .as_str()
        .unwrap()
        .to_owned();
    let old_path = old_url.strip_prefix("http://localhost").unwrap().to_owned();
    assert_eq!(
        send(&env.router, get_request(&old_path, None))
            .await
            .status(),
        StatusCode::OK
    );

    assert_eq!(
        delete(&env, &d.owner, d.card).await.status(),
        StatusCode::NO_CONTENT
    );
    // Gone for the recipient and the owner, and from the feed.
    assert_hidden(&env, &d.friend, d.card).await;
    assert_hidden(&env, &d.owner, d.card).await;
    let feed = json_body(call(&env, "GET", "/v1/feed", &d.friend, None).await).await;
    assert!(feed["items"].as_array().unwrap().is_empty());
    let status: String = sqlx::query_scalar("SELECT status FROM cards.cards WHERE id = $1")
        .bind(d.card)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(status, CardStatus::Deleted.as_str());

    // The event drives the purge; after it the old URL resolves to nothing.
    let (id, payload): (i64, serde_json::Value) = sqlx::query_as(
        "SELECT id, payload FROM platform.outbox_events WHERE topic = 'cards.deleted' AND payload->>'card_id' = $1",
    )
    .bind(d.card.to_string())
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(payload["recipient_ids"], serde_json::json!([d.friend.id]));
    let job = PurgeOnDelete
        .job(&StoredEvent {
            id,
            topic: "cards.deleted".to_owned(),
            payload,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
        })
        .unwrap();
    assert_eq!(job.kind, cards::PURGE_JOB);
    let clean_before = env.media_store.count(Bucket::Clean);
    let purged = env
        .state
        .cards
        .purge_media(&env.state.media, d.card)
        .await
        .unwrap();
    assert_eq!(purged, 3, "render, mask and original");
    assert!(env.media_store.count(Bucket::Clean) < clean_before);
    let gone = send(&env.router, get_request(&old_path, None)).await;
    assert_problem(gone, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    let states: Vec<String> = sqlx::query_scalar(
        "SELECT m.state FROM media.media_objects m JOIN cards.cards c
           ON m.id IN (c.render_media_id, c.mask_media_id, c.original_media_id)
         WHERE c.id = $1",
    )
    .bind(d.card)
    .fetch_all(env.identity.pool())
    .await
    .unwrap();
    assert!(states.iter().all(|s| s == "deleted"), "{states:?}");
    // Purging again is a no-op.
    assert_eq!(
        env.state
            .cards
            .purge_media(&env.state.media, d.card)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn only_the_owner_can_delete_and_others_get_the_missing_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let d = delivered_card(&env).await;
    let stranger = adult(&env).await;
    for user in [&d.friend, &stranger] {
        let denied = delete(&env, user, d.card).await;
        assert_eq!(denied.status(), StatusCode::NOT_FOUND);
        let denied = without_request_id(body_bytes(denied).await);
        let missing = delete(&env, user, Uuid::now_v7()).await;
        assert_eq!(without_request_id(body_bytes(missing).await), denied);
    }
    // Still visible to the recipient: nothing happened.
    assert_eq!(read(&env, &d.friend, d.card).await.status(), StatusCode::OK);
    // Deleting twice: the second is a 404.
    assert_eq!(
        delete(&env, &d.owner, d.card).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        delete(&env, &d.owner, d.card).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn media_shared_with_another_live_card_is_kept() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let d = delivered_card(&env).await;
    // A second card reusing the same render.
    let render: Uuid = sqlx::query_scalar("SELECT render_media_id FROM cards.cards WHERE id = $1")
        .bind(d.card)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    let body = crate::cards_send::card_body(render, None, serde_json::json!({ "mode": "all" }));
    let second = crate::cards_send::post_card(&env, &d.owner, &body).await;
    assert_eq!(second.status(), StatusCode::ACCEPTED);
    assert_eq!(
        delete(&env, &d.owner, d.card).await.status(),
        StatusCode::NO_CONTENT
    );
    let purged = env
        .state
        .cards
        .purge_media(&env.state.media, d.card)
        .await
        .unwrap();
    assert_eq!(purged, 2, "mask and original only");
    let state: String = sqlx::query_scalar("SELECT state FROM media.media_objects WHERE id = $1")
        .bind(render)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(state, "clean");
}
