//! T-143: reports about one target form one queue item, ordered by priority and reporter
//! reputation; reports never act on their own (SR-31).

use crate::boards::new_board;
use crate::common::*;
use crate::public_links::{created_link, token_of};
use crate::reports::{reported, target};
use crate::social_support::*;
use moderation::{ModerationQueue, TargetType};
use time::Duration;
use uuid::Uuid;

fn queue(env: &TestEnv) -> ModerationQueue {
    ModerationQueue::new(env.identity.pool().clone())
}

/// A new owner's live public link: owner, link id, token.
async fn link(env: &TestEnv) -> (User, Uuid, String) {
    let owner = adult(env).await;
    let board = new_board(env, &owner).await;
    let link = created_link(env, &owner, board, "7d").await;
    let id = Uuid::parse_str(link["id"].as_str().unwrap()).unwrap();
    (owner, id, token_of(&link))
}

#[tokio::test]
async fn fifty_reports_from_new_accounts_are_one_item_and_no_suspension() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (owner, link_id, token) = link(&env).await;
    for _ in 0..50 {
        let reporter = adult(&env).await;
        reported(&env, &reporter, &target("public_link", &token, "spam")).await;
    }
    let item = queue(&env)
        .live_for(TargetType::PublicLink, link_id)
        .await
        .unwrap()
        .expect("one live item");
    assert_eq!(item.report_count, 50);
    assert_eq!(item.status, "open");
    assert_eq!(item.target_owner_id, owner.id);
    // Brand-new accounts weigh 0.25 each (config/moderation.yaml).
    assert!((item.weight - 12.5).abs() < 0.01, "{}", item.weight);
    let items: i64 =
        sqlx::query_scalar("SELECT count(*) FROM moderation.queue_items WHERE target_id = $1")
            .bind(link_id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(items, 1);
    let linked: i64 =
        sqlx::query_scalar("SELECT count(*) FROM moderation.reports WHERE queue_item_id = $1")
            .bind(item.id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(linked, 50);

    // No automatic action: the owner is active and the link still serves.
    let status: String = sqlx::query_scalar("SELECT status FROM identity.accounts WHERE id = $1")
        .bind(owner.id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(status, "active");
    let revoked: Option<time::OffsetDateTime> =
        sqlx::query_scalar("SELECT revoked_at FROM sharing.public_links WHERE id = $1")
            .bind(link_id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(revoked, None);
}

#[tokio::test]
async fn a_repeated_report_adds_no_weight() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, link_id, token) = link(&env).await;
    let reporter = adult(&env).await;
    for reason in ["spam", "hate", "other"] {
        reported(&env, &reporter, &target("public_link", &token, reason)).await;
    }
    let item = queue(&env)
        .live_for(TargetType::PublicLink, link_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(item.report_count, 1);
    assert_eq!(item.priority, 20, "only the first report counts");
}

#[tokio::test]
async fn the_queue_orders_by_priority_then_reputation() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, spam_link, spam_token) = link(&env).await;
    let (_, minor_link, minor_token) = link(&env).await;
    let (_, light_link, light_token) = link(&env).await;
    let (_, heavy_link, heavy_token) = link(&env).await;

    let newcomer = adult(&env).await;
    // An established reporter: account older than new_account_days.
    let veteran = adult(&env).await;
    sqlx::query("UPDATE identity.accounts SET created_at = $2 WHERE id = $1")
        .bind(veteran.id)
        .bind(time::OffsetDateTime::now_utc() - Duration::days(30))
        .execute(env.identity.pool())
        .await
        .unwrap();
    let veteran = signed_in_again(&env, &veteran).await;

    reported(&env, &newcomer, &target("public_link", &spam_token, "spam")).await;
    reported(
        &env,
        &newcomer,
        &target("public_link", &minor_token, "minor_safety"),
    )
    .await;
    reported(
        &env,
        &newcomer,
        &target("public_link", &light_token, "hate"),
    )
    .await;
    reported(&env, &veteran, &target("public_link", &heavy_token, "hate")).await;

    let heavy = queue(&env)
        .live_for(TargetType::PublicLink, heavy_link)
        .await
        .unwrap()
        .unwrap();
    assert!((heavy.weight - 1.0).abs() < 0.01, "{}", heavy.weight);

    let ours = [minor_link, heavy_link, light_link, spam_link];
    let order: Vec<Uuid> = queue(&env)
        .live(200)
        .await
        .unwrap()
        .into_iter()
        .map(|item| item.target_id)
        .filter(|id| ours.contains(id))
        .collect();
    // The shared database may hold more items than fit in one page; ours keep their relative order.
    let positions: Vec<usize> = ours
        .iter()
        .filter_map(|id| order.iter().position(|o| o == id))
        .collect();
    let mut sorted = positions.clone();
    sorted.sort_unstable();
    assert_eq!(positions, sorted, "minor safety, then weight, then spam");
}

#[tokio::test]
async fn dismissed_history_lowers_and_actioned_history_raises_the_weight() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let reporter = adult(&env).await;
    sqlx::query("UPDATE identity.accounts SET created_at = $2 WHERE id = $1")
        .bind(reporter.id)
        .bind(time::OffsetDateTime::now_utc() - Duration::days(30))
        .execute(env.identity.pool())
        .await
        .unwrap();
    let reporter = signed_in_again(&env, &reporter).await;
    // Three earlier reports, all dismissed.
    for _ in 0..3 {
        let (_, _, token) = link(&env).await;
        reported(&env, &reporter, &target("public_link", &token, "spam")).await;
    }
    sqlx::query("UPDATE moderation.reports SET status = 'dismissed' WHERE reporter_id = $1")
        .bind(reporter.id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let (_, link_id, token) = link(&env).await;
    reported(&env, &reporter, &target("public_link", &token, "spam")).await;
    let item = queue(&env)
        .live_for(TargetType::PublicLink, link_id)
        .await
        .unwrap()
        .unwrap();
    assert!((item.weight - 0.25).abs() < 0.01, "{}", item.weight);

    sqlx::query("UPDATE moderation.reports SET status = 'actioned' WHERE reporter_id = $1")
        .bind(reporter.id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let (_, link_id, token) = link(&env).await;
    reported(&env, &reporter, &target("public_link", &token, "spam")).await;
    let item = queue(&env)
        .live_for(TargetType::PublicLink, link_id)
        .await
        .unwrap()
        .unwrap();
    assert!((item.weight - 3.0).abs() < 0.01, "capped: {}", item.weight);
}
