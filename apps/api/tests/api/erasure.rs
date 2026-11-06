//! Hard erase after the grace period (T-152, US-29 AC-29.1): idempotent, rows and stored
//! objects gone, a tombstone kept, and erasures replayed after a backup restore.

use crate::cards_read::deliver_to;
use crate::common::*;
use crate::deletion::request_deletion;
use crate::reauth::reauth_token;
use crate::social_support::*;
use http::StatusCode;
use media::{Bucket, ObjectKey, ObjectStore};
use time::Duration;
use uuid::Uuid;

struct Deleted {
    user: User,
    friend: User,
    card: Uuid,
    media_keys: Vec<String>,
}

/// An account with a delivered card and a friend, whose deletion was just requested.
async fn deleted(env: &TestEnv) -> Deleted {
    let user = adult(env).await;
    let friend = adult(env).await;
    make_friends(env, user.id, friend.id).await;
    let card = deliver_to(env, &user, &[&friend]).await;
    let media_keys: Vec<String> =
        sqlx::query_scalar("SELECT storage_key FROM media.media_objects WHERE owner_id = $1")
            .bind(user.id)
            .fetch_all(env.identity.pool())
            .await
            .unwrap();
    let reauth = reauth_token(env, &user).await;
    assert_eq!(
        request_deletion(env, &user, Some(&reauth)).await.status(),
        StatusCode::ACCEPTED
    );
    Deleted {
        user,
        friend,
        card,
        media_keys,
    }
}

async fn count(env: &TestEnv, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap()
}

async fn tombstone(env: &TestEnv, account: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM privacy.deletion_requests WHERE account_id = $1")
        .bind(account)
        .fetch_one(env.identity.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn accounts_are_erased_only_after_the_grace_period_and_idempotently() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let d = deleted(&env).await;
    let service = env.state.deletion();
    assert!(
        !service.erase_account(d.user.id).await.unwrap(),
        "still in grace"
    );
    assert_eq!(
        count(
            &env,
            "SELECT count(*) FROM identity.accounts WHERE id = $1",
            d.user.id
        )
        .await,
        1
    );

    env.clock.advance(Duration::days(7) + Duration::seconds(1));
    // By id: `erase_due` would also take other tests' accounts (shared database, own clocks).
    assert!(service.erase_account(d.user.id).await.unwrap());
    assert_eq!(tombstone(&env, d.user.id).await, "done");
    for sql in [
        "SELECT count(*) FROM identity.accounts WHERE id = $1",
        "SELECT count(*) FROM identity.auth_identities WHERE account_id = $1",
        "SELECT count(*) FROM cards.cards WHERE owner_id = $1",
        "SELECT count(*) FROM media.media_objects WHERE owner_id = $1",
        "SELECT count(*) FROM social.friendships WHERE user_a = $1 OR user_b = $1",
    ] {
        assert_eq!(count(&env, sql, d.user.id).await, 0, "{sql}");
    }
    for key in &d.media_keys {
        let key = ObjectKey::parse(key).unwrap();
        for bucket in [Bucket::Clean, Bucket::Quarantine] {
            assert!(env.media_store.get(bucket, &key).await.unwrap().is_none());
        }
    }
    assert_eq!(
        count(
            &env,
            "SELECT count(*) FROM platform.audit_events WHERE action = 'privacy.account_erased'
             AND target = 'account:' || $1::text",
            d.user.id
        )
        .await,
        1
    );
    // Idempotent: nothing left to do.
    assert!(!service.erase_account(d.user.id).await.unwrap());
    // The friend is untouched; the erased card is gone from their side too.
    assert_eq!(
        count(
            &env,
            "SELECT count(*) FROM identity.accounts WHERE id = $1",
            d.friend.id
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &env,
            "SELECT count(*) FROM cards.card_recipients WHERE card_id = $1",
            d.card
        )
        .await,
        0
    );
}

#[tokio::test]
async fn a_cancelled_deletion_is_never_erased() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let d = deleted(&env).await;
    let back = crate::social_support::signed_in_again(&env, &d.user).await;
    // The grace session can cancel (its own session was started after the request).
    let cancelled = call(&env, "DELETE", "/v1/me/deletion", &back, None).await;
    assert_eq!(cancelled.status(), StatusCode::NO_CONTENT);
    env.clock.advance(Duration::days(30));
    let service = env.state.deletion();
    assert!(!service.erase_account(d.user.id).await.unwrap());
    assert_eq!(tombstone(&env, d.user.id).await, "cancelled");
    assert_eq!(
        count(
            &env,
            "SELECT count(*) FROM identity.accounts WHERE id = $1",
            d.user.id
        )
        .await,
        1
    );
}

#[tokio::test]
async fn erasures_are_replayed_after_a_restore() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let d = deleted(&env).await;
    env.clock.advance(Duration::days(7) + Duration::seconds(1));
    let service = env.state.deletion();
    assert!(service.erase_account(d.user.id).await.unwrap());

    // Restore drill: an older backup brings the account, its profile and a friendship back.
    sqlx::query(
        "INSERT INTO identity.accounts (id, status, created_at, updated_at)
         VALUES ($1, 'active', now(), now())",
    )
    .bind(d.user.id)
    .execute(env.identity.pool())
    .await
    .unwrap();
    sqlx::query("INSERT INTO identity.profiles (account_id, display_name) VALUES ($1, 'Restored')")
        .bind(d.user.id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    make_friends(&env, d.user.id, d.friend.id).await;

    assert!(service.replay_erasures().await.unwrap() >= 1);
    for sql in [
        "SELECT count(*) FROM identity.accounts WHERE id = $1",
        "SELECT count(*) FROM identity.profiles WHERE account_id = $1",
        "SELECT count(*) FROM social.friendships WHERE user_a = $1 OR user_b = $1",
    ] {
        assert_eq!(count(&env, sql, d.user.id).await, 0, "{sql}");
    }
    assert_eq!(tombstone(&env, d.user.id).await, "done");
}
