//! Public snapshots (T-130, AC-24.3, AC-24.5, AC-24.7): only the owner's own active cards, as
//! watermarked derivatives of at most 1080 px; every friend's card is a neutral tile with no id or
//! media; a watermarked 1200x630 OG image; idempotent rebuilds.

use std::sync::Arc;

use crate::board_ops::{add_card, apply};
use crate::boards::new_board;
use crate::cards_read::deliver_to;
use crate::common::*;
use crate::public_links::created_link;
use crate::social_support::*;
use media::{Bucket, ObjectKey, ObjectStore};
use platform::outbox::{StoredEvent, Subscriber};
use serde_json::{Value, json};
use sharing::{SnapshotBuilder, SnapshotOnEvents, SnapshotOutcome};
use uuid::Uuid;

struct Setup {
    owner: User,
    friend: User,
    board: Uuid,
    own_card: Uuid,
    friend_card: Uuid,
}

/// A board of `owner` with one of their own cards and one card a friend sent them, and a link.
async fn setup(env: &TestEnv) -> Setup {
    let owner = adult(env).await;
    let friend = adult(env).await;
    make_friends(env, owner.id, friend.id).await;
    let own_card = deliver_to(env, &owner, &[&friend]).await;
    let friend_card = deliver_to(env, &friend, &[&owner]).await;
    let board = new_board(env, &owner).await;
    let body = apply(
        env,
        &owner,
        board,
        json!([
            add_card(Uuid::now_v7(), own_card),
            add_card(Uuid::now_v7(), friend_card)
        ]),
    )
    .await;
    assert!(
        body["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "applied"),
        "{body}"
    );
    created_link(env, &owner, board, "7d").await;
    Setup {
        owner,
        friend,
        board,
        own_card,
        friend_card,
    }
}

fn builder(env: &TestEnv) -> SnapshotBuilder {
    SnapshotBuilder::new(
        env.identity.pool().clone(),
        env.clock.clone(),
        Arc::clone(&env.state.boards),
        Arc::clone(&env.state.media),
    )
}

async fn snapshot_row(env: &TestEnv, board: Uuid) -> Option<(Value, String)> {
    sqlx::query_as("SELECT snapshot, og_key FROM sharing.public_snapshots WHERE board_id = $1")
        .bind(board)
        .fetch_optional(env.identity.pool())
        .await
        .unwrap()
}

/// Every storage key of the card's render and mask (full size and derivatives).
async fn media_keys_of(env: &TestEnv, card: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT d.storage_key FROM cards.cards c
         JOIN media.media_derivatives d ON d.media_id IN (c.render_media_id, c.mask_media_id)
         WHERE c.id = $1
         UNION
         SELECT m.storage_key FROM cards.cards c
         JOIN media.media_objects m ON m.id IN (c.render_media_id, c.mask_media_id)
         WHERE c.id = $1",
    )
    .bind(card)
    .fetch_all(env.identity.pool())
    .await
    .unwrap()
}

async fn object(env: &TestEnv, key: &str) -> Option<bytes::Bytes> {
    env.media_store
        .get(Bucket::Clean, &ObjectKey::parse(key).unwrap())
        .await
        .unwrap()
}

fn pins(snapshot: &Value) -> Vec<Value> {
    snapshot["pins"].as_array().unwrap().clone()
}

#[tokio::test]
async fn friends_cards_never_reach_the_snapshot_or_og() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    // Creating the link enqueued the first snapshot.
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.jobs WHERE kind = 'sharing.snapshot' AND payload->>'board_id' = $1",
    )
    .bind(s.board.to_string())
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(queued, 1);

    assert_eq!(
        builder(&env).rebuild(s.board).await.unwrap(),
        SnapshotOutcome::Built
    );
    let (snapshot, og_key) = snapshot_row(&env, s.board).await.unwrap();
    let mut kinds: Vec<String> = pins(&snapshot)
        .iter()
        .map(|p| p["kind"].as_str().unwrap().to_owned())
        .collect();
    kinds.sort_unstable();
    assert_eq!(kinds, vec!["card", "private_tile"]);
    let card_pin = pins(&snapshot)
        .into_iter()
        .find(|p| p["kind"] == "card")
        .unwrap();
    assert_eq!(card_pin["card_id"], s.own_card.to_string());
    let tile = pins(&snapshot)
        .into_iter()
        .find(|p| p["kind"] == "private_tile")
        .unwrap();
    for field in ["card_id", "media_key", "width", "height"] {
        assert!(
            tile.get(field).is_none(),
            "{field} on a private tile: {tile}"
        );
    }

    // AC-24.3: nothing of the friend's card or account anywhere in the document.
    let text = snapshot.to_string();
    assert!(!text.contains(&s.friend_card.to_string()));
    assert!(!text.contains(&s.friend.id.to_string()));
    for key in media_keys_of(&env, s.friend_card).await {
        assert!(!text.contains(&key), "friend media key in snapshot");
    }
    // The owner's original media keys are not exposed either: only the watermarked copy.
    for key in media_keys_of(&env, s.own_card).await {
        assert!(!text.contains(&key), "unwatermarked media key in snapshot");
    }
    let public_cards: Vec<Uuid> =
        sqlx::query_scalar("SELECT card_id FROM sharing.public_media WHERE owner_id = ANY($1)")
            .bind(vec![s.owner.id, s.friend.id])
            .fetch_all(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(public_cards, vec![s.own_card]);

    // AC-24.5: the public image is a watermarked derivative, at most 1080 px.
    let key = card_pin["media_key"].as_str().unwrap();
    let bytes = object(&env, key).await.unwrap();
    let image = image::load_from_memory(&bytes).unwrap();
    assert!(image.width() <= 1080 && image.height() <= 1080);
    // AC-24.7: the OG image is 1200x630.
    let og = image::load_from_memory(&object(&env, &og_key).await.unwrap()).unwrap();
    assert_eq!((og.width(), og.height()), (1200, 630));
}

#[tokio::test]
async fn rebuilds_are_idempotent() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let builder = builder(&env);
    assert_eq!(
        builder.rebuild(s.board).await.unwrap(),
        SnapshotOutcome::Built
    );
    let (first, og_key) = snapshot_row(&env, s.board).await.unwrap();
    let objects = env.media_store.count(Bucket::Clean);
    assert_eq!(
        builder.rebuild(s.board).await.unwrap(),
        SnapshotOutcome::Unchanged
    );
    let (second, og_again) = snapshot_row(&env, s.board).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(og_key, og_again);
    assert_eq!(
        env.media_store.count(Bucket::Clean),
        objects,
        "nothing new stored"
    );

    // A change rebuilds with a new OG image and drops the old one; the card image is reused.
    let card_key = pins(&first)
        .into_iter()
        .find(|p| p["kind"] == "card")
        .unwrap()["media_key"]
        .clone();
    let body = apply(
        &env,
        &s.owner,
        s.board,
        json!([add_card(Uuid::now_v7(), s.own_card)]),
    )
    .await;
    assert_eq!(body["results"][0]["status"], "applied", "{body}");
    assert_eq!(
        builder.rebuild(s.board).await.unwrap(),
        SnapshotOutcome::Built
    );
    let (third, new_og) = snapshot_row(&env, s.board).await.unwrap();
    assert_ne!(new_og, og_key);
    assert!(
        object(&env, &og_key).await.is_none(),
        "old OG image deleted"
    );
    let card_keys: Vec<Value> = pins(&third)
        .into_iter()
        .filter(|p| p["kind"] == "card")
        .map(|p| p["media_key"].clone())
        .collect();
    assert_eq!(card_keys, vec![card_key.clone(), card_key]);
}

#[tokio::test]
async fn held_cards_are_tiles_and_boards_without_live_links_get_nothing() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    sqlx::query("UPDATE cards.cards SET status = 'held_review' WHERE id = $1")
        .bind(s.own_card)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let builder = builder(&env);
    assert_eq!(
        builder.rebuild(s.board).await.unwrap(),
        SnapshotOutcome::Built
    );
    let (snapshot, _) = snapshot_row(&env, s.board).await.unwrap();
    assert!(
        pins(&snapshot).iter().all(|p| p["kind"] == "private_tile"),
        "{snapshot}"
    );
    assert!(!snapshot.to_string().contains(&s.own_card.to_string()));

    // Another board of the owner without a link is never snapshotted.
    let other = new_board(&env, &s.owner).await;
    assert_eq!(
        builder.rebuild(other).await.unwrap(),
        SnapshotOutcome::NoLiveLink
    );
    assert!(snapshot_row(&env, other).await.is_none());
}

#[tokio::test]
async fn a_deleted_card_leaves_the_snapshot_and_its_public_image_is_deleted() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let builder = builder(&env);
    builder.rebuild(s.board).await.unwrap();
    let key: String =
        sqlx::query_scalar("SELECT object_key FROM sharing.public_media WHERE card_id = $1")
            .bind(s.own_card)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    let deleted = call(
        &env,
        "DELETE",
        &format!("/v1/cards/{}", s.own_card),
        &s.owner,
        None,
    )
    .await;
    assert_eq!(deleted.status(), http::StatusCode::NO_CONTENT);
    builder.card_deleted(s.own_card).await.unwrap();
    let (snapshot, _) = snapshot_row(&env, s.board).await.unwrap();
    assert!(
        !snapshot.to_string().contains(&s.own_card.to_string()),
        "{snapshot}"
    );
    assert!(pins(&snapshot).iter().all(|p| p["kind"] != "card"));
    assert!(object(&env, &key).await.is_none());
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sharing.public_media WHERE card_id = $1")
            .bind(s.own_card)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn board_changes_and_card_deletions_schedule_snapshots() {
    let event = |topic: &str, payload: Value| StoredEvent {
        id: 1,
        topic: topic.to_owned(),
        payload,
        created_at: time::OffsetDateTime::UNIX_EPOCH,
    };
    let board = Uuid::now_v7();
    let job = SnapshotOnEvents
        .job(&event(
            boards::BOARD_UPDATED,
            json!({ "board_id": board, "version": 3, "member_ids": [] }),
        ))
        .unwrap();
    assert_eq!(job.kind, sharing::SNAPSHOT_JOB);
    assert_eq!(job.payload, json!({ "board_id": board }));
    let card = Uuid::now_v7();
    let job = SnapshotOnEvents
        .job(&event(cards::CARD_DELETED, json!({ "card_id": card })))
        .unwrap();
    assert_eq!(job.payload, json!({ "card_id": card }));
    assert!(SnapshotOnEvents.topics().contains(&cards::CARD_DELETED));
}
