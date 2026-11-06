//! PersonalData per module (T-150, US-29, US-30): every table is covered by a module or exempt
//! with a reason; each module exports the account's data and erases it (rows and stored objects)
//! without touching other accounts.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::board_invitations::join;
use crate::board_ops::{add_card, apply};
use crate::boards::new_board;
use crate::cards_read::deliver_to;
use crate::common::*;
use crate::public_links::created_link;
use crate::social_support::*;
use media::{Bucket, ObjectStore};
use serde_json::Value;
use sharing::SnapshotBuilder;
use uuid::Uuid;

#[tokio::test]
async fn every_table_is_covered_by_a_module_or_exempt() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_schema || '.' || table_name FROM information_schema.tables
         WHERE table_type = 'BASE TABLE'
           AND table_schema NOT IN ('pg_catalog', 'information_schema', 'public')",
    )
    .fetch_all(env.identity.pool())
    .await
    .unwrap();
    let registry = api::privacy::registry(&env.state);
    let covered: BTreeSet<&str> = registry.covered_tables().into_iter().collect();
    let exempt: BTreeSet<&str> = api::privacy::EXEMPT_TABLES
        .iter()
        .map(|(t, _)| *t)
        .collect();
    assert!(
        covered.is_disjoint(&exempt),
        "a table is both covered and exempt"
    );
    let missing: Vec<&String> = tables
        .iter()
        .filter(|t| !covered.contains(t.as_str()) && !exempt.contains(t.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "tables without a PersonalData implementation (add one, or an exemption with a reason \
         in apps/api/src/privacy.rs): {missing:?}"
    );
    let known: BTreeSet<&str> = tables.iter().map(String::as_str).collect();
    for table in covered.iter().chain(exempt.iter()) {
        assert!(
            known.contains(table),
            "{table} is declared but does not exist"
        );
    }
    let mut modules: Vec<&str> = registry.modules().iter().map(|m| m.module()).collect();
    assert_eq!(modules[0], "identity", "identity erases last");
    modules.sort_unstable();
    modules.dedup();
    assert_eq!(
        modules.len(),
        registry.modules().len(),
        "module names are unique"
    );
}

struct World {
    me: User,
    friend: User,
    blocked: User,
    my_card: Uuid,
    their_card: Uuid,
    my_board: Uuid,
    their_board: Uuid,
}

/// An account with data in every module, plus a friend whose data must survive.
async fn world(env: &TestEnv) -> World {
    let me = adult(env).await;
    let friend = adult(env).await;
    let blocked = adult(env).await;
    make_friends(env, me.id, friend.id).await;
    block(env, me.id, blocked.id).await;
    let my_card = deliver_to(env, &me, &[&friend]).await;
    let their_card = deliver_to(env, &friend, &[&me]).await;
    sqlx::query("INSERT INTO cards.reactions (card_id, reactor_id, kind, updated_at) VALUES ($1, $2, 'react.love', now())")
        .bind(their_card)
        .bind(me.id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let my_board = new_board(env, &me).await;
    apply(
        env,
        &me,
        my_board,
        serde_json::json!([add_card(Uuid::now_v7(), my_card)]),
    )
    .await;
    let their_board = new_board(env, &friend).await;
    join(env, &friend, their_board, &me).await;
    apply(
        env,
        &me,
        their_board,
        serde_json::json!([add_card(Uuid::now_v7(), my_card)]),
    )
    .await;
    created_link(env, &me, my_board, "never").await;
    SnapshotBuilder::new(
        env.identity.pool().clone(),
        env.clock.clone(),
        Arc::clone(&env.state.boards),
        Arc::clone(&env.state.media),
    )
    .rebuild(my_board)
    .await
    .unwrap();
    World {
        me,
        friend,
        blocked,
        my_card,
        their_card,
        my_board,
        their_board,
    }
}

fn keys_of(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                out.push(key.clone());
                keys_of(inner, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| keys_of(i, out)),
        _ => {}
    }
}

#[tokio::test]
async fn each_module_exports_the_accounts_data() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let w = world(&env).await;
    let export = api::privacy::registry(&env.state)
        .export(w.me.id)
        .await
        .unwrap();
    let data = |module: &str| export[module].data.clone();

    let identity = data("identity");
    assert_eq!(identity["account"]["id"], w.me.id.to_string());
    assert!(
        identity["account"]["birth_date"]
            .as_str()
            .is_some_and(|d| d.len() == 10)
    );
    assert_eq!(identity["profile"]["handle"], w.me.handle.as_str());

    let social = data("social");
    assert_eq!(social["friends"][0]["handle"], w.friend.handle.as_str());
    assert_eq!(
        social["blocks"][0]["blocked_user_id"],
        w.blocked.id.to_string()
    );

    let cards = data("cards");
    assert_eq!(cards["sent"][0]["id"], w.my_card.to_string());
    assert_eq!(
        cards["sent"][0]["recipient_ids"][0],
        w.friend.id.to_string()
    );
    assert_eq!(cards["received"][0]["card_id"], w.their_card.to_string());
    assert_eq!(cards["reactions"][0]["kind"], "react.love");

    let media = &export["media"];
    assert!(media.data["objects"].as_array().unwrap().len() >= 3);
    assert!(
        media
            .files
            .iter()
            .any(|f| f.path.starts_with("media/card_render/"))
    );
    assert!(
        media
            .files
            .iter()
            .any(|f| f.path.starts_with("media/original/"))
    );

    let boards = data("boards");
    assert_eq!(boards["owned"][0]["id"], w.my_board.to_string());
    assert_eq!(boards["owned"][0]["pins"].as_array().unwrap().len(), 1);
    assert_eq!(
        boards["memberships"][0]["board_id"],
        w.their_board.to_string()
    );
    assert_eq!(boards["pins_on_other_boards"].as_array().unwrap().len(), 1);

    assert_eq!(data("sharing")["public_links"].as_array().unwrap().len(), 1);
    assert!(export.contains_key("notifications") && export.contains_key("platform"));

    // No secret ever leaves through an export: no hashes, ciphertexts or tokens.
    let mut keys = Vec::new();
    for section in export.values() {
        keys_of(&section.data, &mut keys);
    }
    for key in keys {
        assert!(
            !key.contains("hash") && !key.ends_with("_enc") && !key.contains("token"),
            "secret-looking key {key} in the export"
        );
    }
}

async fn count(env: &TestEnv, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn erase_removes_rows_and_objects_and_spares_other_accounts() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let w = world(&env).await;
    let public_image: String =
        sqlx::query_scalar("SELECT object_key FROM sharing.public_media WHERE card_id = $1")
            .bind(w.my_card)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    let my_media: Vec<String> =
        sqlx::query_scalar("SELECT storage_key FROM media.media_objects WHERE owner_id = $1")
            .bind(w.me.id)
            .fetch_all(env.identity.pool())
            .await
            .unwrap();
    let registry = api::privacy::registry(&env.state);
    registry.erase(w.me.id).await.unwrap();
    // Idempotent.
    registry.erase(w.me.id).await.unwrap();

    for sql in [
        "SELECT count(*) FROM identity.accounts WHERE id = $1",
        "SELECT count(*) FROM identity.profiles WHERE account_id = $1",
        "SELECT count(*) FROM social.friendships WHERE user_a = $1 OR user_b = $1",
        "SELECT count(*) FROM social.blocks WHERE blocker_id = $1",
        "SELECT count(*) FROM media.media_objects WHERE owner_id = $1",
        "SELECT count(*) FROM cards.cards WHERE owner_id = $1",
        "SELECT count(*) FROM cards.reactions WHERE reactor_id = $1",
        "SELECT count(*) FROM cards.card_recipients WHERE recipient_id = $1",
        "SELECT count(*) FROM boards.boards WHERE owner_id = $1",
        "SELECT count(*) FROM boards.board_members WHERE user_id = $1",
        "SELECT count(*) FROM boards.pins WHERE created_by = $1",
        "SELECT count(*) FROM boards.board_ops WHERE actor_id = $1",
        "SELECT count(*) FROM sharing.public_links WHERE owner_id = $1",
        "SELECT count(*) FROM sharing.public_snapshots WHERE owner_id = $1",
        "SELECT count(*) FROM sharing.public_media WHERE owner_id = $1",
        "SELECT count(*) FROM platform.idempotency_keys WHERE user_id = $1",
    ] {
        assert_eq!(count(&env, sql, w.me.id).await, 0, "{sql}");
    }
    assert!(
        env.media_store
            .get(
                Bucket::Clean,
                &media::ObjectKey::parse(&public_image).unwrap()
            )
            .await
            .unwrap()
            .is_none(),
        "public image deleted"
    );
    for key in my_media {
        let key = media::ObjectKey::parse(&key).unwrap();
        assert!(
            env.media_store
                .get(Bucket::Clean, &key)
                .await
                .unwrap()
                .is_none()
        );
    }

    // The friend keeps their account, card, board and membership list.
    assert_eq!(
        count(
            &env,
            "SELECT count(*) FROM identity.accounts WHERE id = $1",
            w.friend.id
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &env,
            "SELECT count(*) FROM cards.cards WHERE id = $1",
            w.their_card
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &env,
            "SELECT count(*) FROM boards.boards WHERE id = $1",
            w.their_board
        )
        .await,
        1
    );
}

#[tokio::test]
async fn hide_revokes_public_links_and_schedules_their_purge() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let w = world(&env).await;
    api::privacy::registry(&env.state)
        .hide(w.me.id)
        .await
        .unwrap();
    let reasons: Vec<Option<String>> =
        sqlx::query_scalar("SELECT revoked_reason FROM sharing.public_links WHERE owner_id = $1")
            .bind(w.me.id)
            .fetch_all(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(reasons, vec![Some("owner_ineligible".to_owned())]);
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.jobs j JOIN sharing.public_links l
           ON j.payload->>'link_id' = l.id::text
         WHERE j.kind = 'sharing.purge' AND l.owner_id = $1",
    )
    .bind(w.me.id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(queued, 1);
}
