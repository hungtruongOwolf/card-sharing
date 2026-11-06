//! CDN purge of ended public links (T-133, AC-24.4): revoke, expiry and board deletion purge the
//! link's cache tag and delete the board's public objects once no live link needs them.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::board_ops::{add_card, apply};
use crate::boards::new_board;
use crate::cards_read::deliver_to;
use crate::common::*;
use crate::public_links::{created_link, token_of};
use crate::social_support::*;
use axum::body::Body;
use http::{Request, StatusCode};
use media::{Bucket, ObjectKey, ObjectStore};
use platform::outbox::{StoredEvent, Subscriber};
use serde_json::{Value, json};
use sharing::{FakeCdnPurger, PurgeOnBoardDeleted, Purger, SnapshotBuilder, cache_tag};
use time::Duration;
use uuid::Uuid;

struct Setup {
    owner: User,
    board: Uuid,
    card: Uuid,
    link: Uuid,
    token: String,
}

async fn board_with_card(env: &TestEnv, owner: &User, card: Uuid) -> Uuid {
    let board = new_board(env, owner).await;
    apply(env, owner, board, json!([add_card(Uuid::now_v7(), card)])).await;
    board
}

async fn setup(env: &TestEnv) -> Setup {
    let owner = adult(env).await;
    let friend = adult(env).await;
    make_friends(env, owner.id, friend.id).await;
    let card = deliver_to(env, &owner, &[&friend]).await;
    let board = board_with_card(env, &owner, card).await;
    let link = created_link(env, &owner, board, "7d").await;
    snapshots(env).rebuild(board).await.unwrap();
    Setup {
        owner,
        board,
        card,
        link: Uuid::parse_str(link["id"].as_str().unwrap()).unwrap(),
        token: token_of(&link),
    }
}

fn snapshots(env: &TestEnv) -> SnapshotBuilder {
    SnapshotBuilder::new(
        env.identity.pool().clone(),
        env.clock.clone(),
        Arc::clone(&env.state.boards),
        Arc::clone(&env.state.media),
    )
}

fn purger(env: &TestEnv) -> (Purger, Arc<FakeCdnPurger>) {
    let cdn = Arc::new(FakeCdnPurger::default());
    let purger = Purger::new(
        env.identity.pool().clone(),
        env.clock.clone(),
        Arc::clone(&env.state.media),
        cdn.clone(),
    );
    (purger, cdn)
}

async fn og_key(env: &TestEnv, board: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT og_key FROM sharing.public_snapshots WHERE board_id = $1")
        .bind(board)
        .fetch_optional(env.identity.pool())
        .await
        .unwrap()
}

async fn card_key(env: &TestEnv, card: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT object_key FROM sharing.public_media WHERE card_id = $1")
        .bind(card)
        .fetch_optional(env.identity.pool())
        .await
        .unwrap()
}

async fn exists(env: &TestEnv, key: &str) -> bool {
    env.media_store
        .get(Bucket::Clean, &ObjectKey::parse(key).unwrap())
        .await
        .unwrap()
        .is_some()
}

async fn purged_at(env: &TestEnv, link: Uuid) -> Option<time::OffsetDateTime> {
    sqlx::query_scalar("SELECT purged_at FROM sharing.public_links WHERE id = $1")
        .bind(link)
        .fetch_one(env.identity.pool())
        .await
        .unwrap()
}

async fn revoke(env: &TestEnv, owner: &User, link: Uuid) {
    let response = call(
        env,
        "DELETE",
        &format!("/v1/public-links/{link}"),
        owner,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn revoking_purges_the_cdn_tag_and_deletes_the_public_objects() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let og = og_key(&env, s.board).await.unwrap();
    let image = card_key(&env, s.card).await.unwrap();
    let (purger, cdn) = purger(&env);
    assert!(
        !purger.purge_link(s.link).await.unwrap(),
        "a live link is not purged"
    );

    revoke(&env, &s.owner, s.link).await;
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.jobs WHERE kind = 'sharing.purge' AND payload->>'link_id' = $1",
    )
    .bind(s.link.to_string())
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(queued, 1, "the revoke enqueued the purge");

    assert!(purger.purge_link(s.link).await.unwrap());
    assert_eq!(*cdn.purged.lock().unwrap(), vec![cache_tag(s.link)]);
    assert!(og_key(&env, s.board).await.is_none());
    assert!(!exists(&env, &og).await, "OG image deleted");
    assert!(card_key(&env, s.card).await.is_none());
    assert!(!exists(&env, &image).await, "public card image deleted");
    assert!(purged_at(&env, s.link).await.is_some());
    // Idempotent.
    assert!(!purger.purge_link(s.link).await.unwrap());
    assert_eq!(cdn.purged.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn objects_another_live_link_needs_are_kept() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    // A second link on the same board, and another board showing the same card.
    created_link(&env, &s.owner, s.board, "never").await;
    let other = board_with_card(&env, &s.owner, s.card).await;
    created_link(&env, &s.owner, other, "never").await;
    snapshots(&env).rebuild(other).await.unwrap();
    let image = card_key(&env, s.card).await.unwrap();

    revoke(&env, &s.owner, s.link).await;
    let (purger, cdn) = purger(&env);
    assert!(purger.purge_link(s.link).await.unwrap());
    assert_eq!(*cdn.purged.lock().unwrap(), vec![cache_tag(s.link)]);
    assert!(
        og_key(&env, s.board).await.is_some(),
        "the board still has a live link"
    );
    assert!(exists(&env, &image).await);

    // Deleting the first board revokes its remaining link; the card image stays for `other`.
    let deleted = call(
        &env,
        "DELETE",
        &format!("/v1/boards/{}", s.board),
        &s.owner,
        None,
    )
    .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    purger.board_deleted(s.board).await.unwrap();
    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sharing.public_links WHERE board_id = $1 AND revoked_at IS NULL",
    )
    .bind(s.board)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(live, 0);
    let reasons: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT revoked_reason FROM sharing.public_links WHERE board_id = $1 ORDER BY created_at",
    )
    .bind(s.board)
    .fetch_all(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(
        reasons,
        vec![Some("owner".to_owned()), Some("board_deleted".to_owned())]
    );
    assert!(og_key(&env, s.board).await.is_none());
    assert!(exists(&env, &image).await, "still shown on the other board");
    assert_eq!(cdn.purged.lock().unwrap().len(), 2);
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.audit_events
         WHERE action = 'sharing.link_revoked' AND details->>'reason' = 'board_deleted'
           AND details->>'board_id' = $1",
    )
    .bind(s.board.to_string())
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(audits, 1);
}

#[tokio::test]
async fn expired_links_are_swept_and_purged() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let day = created_link(&env, &s.owner, s.board, "24h").await;
    let day = Uuid::parse_str(day["id"].as_str().unwrap()).unwrap();
    let (purger, cdn) = purger(&env);
    env.clock.advance(Duration::hours(25));
    let swept = purger.sweep().await.unwrap();
    assert!(swept >= 1);
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.jobs WHERE kind = 'sharing.purge' AND payload->>'link_id' = $1",
    )
    .bind(day.to_string())
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(queued, 1);
    assert!(purger.purge_link(day).await.unwrap());
    assert!(cdn.purged.lock().unwrap().contains(&cache_tag(day)));
    assert!(
        og_key(&env, s.board).await.is_some(),
        "the 7d link is still live"
    );
    assert!(!purger.purge_link(s.link).await.unwrap());
}

#[tokio::test]
async fn a_cdn_outage_keeps_the_link_unpurged_for_a_retry() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    revoke(&env, &s.owner, s.link).await;
    let (purger, cdn) = purger(&env);
    cdn.fail.store(true, Ordering::SeqCst);
    assert!(purger.purge_link(s.link).await.is_err());
    assert!(purged_at(&env, s.link).await.is_none());
    assert!(
        og_key(&env, s.board).await.is_some(),
        "nothing deleted before the purge"
    );
    cdn.fail.store(false, Ordering::SeqCst);
    assert!(purger.purge_link(s.link).await.unwrap());
    assert!(purged_at(&env, s.link).await.is_some());
}

#[tokio::test]
async fn public_responses_carry_the_cache_tag() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    for path in [
        format!("/v1/public/boards/{}", s.token),
        format!("/v1/public/boards/{}/og", s.token),
    ] {
        let request = Request::get(&path)
            .header("x-service-credential", TEST_SERVICE_CREDENTIAL)
            .header("x-client-ip", "10.13.3.1")
            .body(Body::empty())
            .unwrap();
        let response = send(&env.router, request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = json_body(response).await;
        assert_eq!(body["cache_tag"], cache_tag(s.link));
    }
}

#[test]
fn board_deletions_schedule_a_purge() {
    let board = Uuid::now_v7();
    let event = StoredEvent {
        id: 1,
        topic: boards::BOARD_DELETED.to_owned(),
        payload: json!({ "board_id": board, "owner_id": Uuid::now_v7() }),
        created_at: time::OffsetDateTime::UNIX_EPOCH,
    };
    let job = PurgeOnBoardDeleted.job(&event).unwrap();
    assert_eq!(job.kind, sharing::PURGE_JOB);
    assert_eq!(job.payload, json!({ "board_id": board }));
}
