//! T-146: moderation actions (US-27 AC-27.1, SR-32, SR-33): every action recorded with actor,
//! reason and time; effects through the owning modules; bans admin-only, two admins for accounts
//! older than 30 days.

use crate::boards::new_board;
use crate::cards_read::delivered_card;
use crate::common::*;
use crate::public_links::{created_link, token_of};
use crate::reports::{reported, target};
use crate::social_support::*;
use crate::staff_auth::{StaffSession, signed_in_staff, staff_member};
use axum::body::Body;
use http::{Request, Response, StatusCode, header};
use identity::StaffRole;
use moderation::{ModerationQueue, TargetType};
use platform::clock::Clock;
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

async fn act(env: &TestEnv, session: &StaffSession, item: Uuid, body: &Value) -> Response<Body> {
    let request = Request::post(format!("/v1/admin/reports/{item}/actions"))
        .header(header::COOKIE, &session.cookie)
        .header("x-csrf-token", &session.csrf)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    send(&env.router, request).await
}

async fn item_for(env: &TestEnv, kind: TargetType, id: Uuid) -> Uuid {
    ModerationQueue::new(env.identity.pool().clone())
        .live_for(kind, id)
        .await
        .unwrap()
        .unwrap()
        .id
}

/// A reported public link: owner, link id and its queue item.
async fn reported_link(env: &TestEnv) -> (User, Uuid, Uuid) {
    let owner = adult(env).await;
    let board = new_board(env, &owner).await;
    let link = created_link(env, &owner, board, "7d").await;
    let link_id = Uuid::parse_str(link["id"].as_str().unwrap()).unwrap();
    let reporter = adult(env).await;
    reported(
        env,
        &reporter,
        &target("public_link", token_of(&link), "spam"),
    )
    .await;
    let item = item_for(env, TargetType::PublicLink, link_id).await;
    (owner, link_id, item)
}

async fn account_status(env: &TestEnv, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM identity.accounts WHERE id = $1")
        .bind(id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap()
}

async fn make_old(env: &TestEnv, id: Uuid) {
    sqlx::query("UPDATE identity.accounts SET created_at = $2 WHERE id = $1")
        .bind(id)
        .bind(OffsetDateTime::now_utc() - Duration::days(40))
        .execute(env.identity.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn dismiss_closes_the_item_and_records_actor_reason_and_time() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    let session = signed_in_staff(&env, &env.router, &moderator).await;
    let (_, link_id, item) = reported_link(&env).await;

    // The queue lists it, and the read is audited.
    let request = Request::get("/v1/admin/reports")
        .header(header::COOKIE, &session.cookie)
        .body(Body::empty())
        .unwrap();
    let list = send(&env.router, request).await;
    assert_eq!(list.status(), StatusCode::OK);
    let views: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.audit_events
         WHERE actor_id = $1 AND actor_kind = 'staff' AND action = 'moderation.staff_view'",
    )
    .bind(moderator.account)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(views, 1);

    let response = act(
        &env,
        &session,
        item,
        &json!({"action": "dismiss", "reason": "not spam"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["action"]["status"], "applied");
    let (actor, action, reason, target_id): (Uuid, String, String, Uuid) = sqlx::query_as(
        "SELECT actor_id, action, reason, target_id FROM moderation.moderation_actions
         WHERE queue_item_id = $1",
    )
    .bind(item)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(
        (actor, action.as_str(), reason.as_str(), target_id),
        (moderator.account, "dismiss", "not spam", link_id)
    );
    let statuses: Vec<String> = sqlx::query_scalar(
        "SELECT status FROM moderation.reports WHERE queue_item_id = $1
         UNION ALL SELECT status FROM moderation.queue_items WHERE id = $1",
    )
    .bind(item)
    .fetch_all(env.identity.pool())
    .await
    .unwrap();
    assert!(statuses.iter().all(|s| s == "dismissed"), "{statuses:?}");
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.audit_events
         WHERE action = 'moderation.action' AND target = $1 AND actor_id = $2",
    )
    .bind(format!("queue_item:{item}"))
    .bind(moderator.account)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(audited, 1);

    // Decided: acting again is the missing 404.
    let again = act(
        &env,
        &session,
        item,
        &json!({"action": "warn", "reason": "late"}),
    )
    .await;
    assert_problem(again, StatusCode::NOT_FOUND, "NOT_FOUND").await;
}

#[tokio::test]
async fn revoke_link_and_remove_content_go_through_the_owning_modules() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    let session = signed_in_staff(&env, &env.router, &moderator).await;

    let (_, link_id, item) = reported_link(&env).await;
    let response = act(
        &env,
        &session,
        item,
        &json!({"action": "revoke_link", "reason": "spam link"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let reason: Option<String> =
        sqlx::query_scalar("SELECT revoked_reason FROM sharing.public_links WHERE id = $1")
            .bind(link_id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(reason.as_deref(), Some("moderator"));

    let delivered = delivered_card(&env).await;
    reported(
        &env,
        &delivered.friend,
        &target("card", delivered.card, "violence"),
    )
    .await;
    let item = item_for(&env, TargetType::Card, delivered.card).await;
    // revoke_link does not fit a card.
    let wrong = act(
        &env,
        &session,
        item,
        &json!({"action": "revoke_link", "reason": "x"}),
    )
    .await;
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);
    let response = act(
        &env,
        &session,
        item,
        &json!({"action": "remove_content", "reason": "gore"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let status: String = sqlx::query_scalar("SELECT status FROM cards.cards WHERE id = $1")
        .bind(delivered.card)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(status, "deleted");
}

#[tokio::test]
async fn suspend_needs_an_end_and_suspends_the_owner() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    let session = signed_in_staff(&env, &env.router, &moderator).await;
    let (owner, _, item) = reported_link(&env).await;
    let missing = act(
        &env,
        &session,
        item,
        &json!({"action": "suspend", "reason": "abuse"}),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    let until = (env.clock.now() + Duration::days(400))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let too_long = act(
        &env,
        &session,
        item,
        &json!({"action": "suspend", "reason": "abuse", "until": until}),
    )
    .await;
    assert_eq!(too_long.status(), StatusCode::BAD_REQUEST);
    let until = (env.clock.now() + Duration::days(7))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let response = act(
        &env,
        &session,
        item,
        &json!({"action": "suspend", "reason": "abuse", "until": until}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(account_status(&env, owner.id).await, "suspended");
    // The owner's session is gone.
    let me = send(&env.router, get_request("/v1/me", Some(&owner.token))).await;
    assert!(
        matches!(
            me.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ),
        "{}",
        me.status()
    );
}

#[tokio::test]
async fn moderators_cannot_ban() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    let session = signed_in_staff(&env, &env.router, &moderator).await;
    let (owner, _, item) = reported_link(&env).await;
    let response = act(
        &env,
        &session,
        item,
        &json!({"action": "ban", "reason": "abuse"}),
    )
    .await;
    assert_problem(response, StatusCode::FORBIDDEN, "FORBIDDEN").await;
    assert_eq!(account_status(&env, owner.id).await, "active");
}

#[tokio::test]
async fn an_admin_bans_a_young_account_at_once() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let admin = staff_member(&env, StaffRole::Admin).await;
    let session = signed_in_staff(&env, &env.router, &admin).await;
    let (owner, _, item) = reported_link(&env).await;
    let response = act(
        &env,
        &session,
        item,
        &json!({"action": "ban", "reason": "spam ring"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(account_status(&env, owner.id).await, "banned");
}

#[tokio::test]
async fn banning_an_older_account_needs_a_second_admin() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let first = staff_member(&env, StaffRole::Admin).await;
    let second = staff_member(&env, StaffRole::Admin).await;
    let first_session = signed_in_staff(&env, &env.router, &first).await;
    let second_session = signed_in_staff(&env, &env.router, &second).await;
    let (owner, _, item) = reported_link(&env).await;
    make_old(&env, owner.id).await;

    let body = json!({"action": "ban", "reason": "repeat offender"});
    let pending = act(&env, &first_session, item, &body).await;
    assert_eq!(pending.status(), StatusCode::ACCEPTED);
    assert_eq!(
        json_body(pending).await["action"]["status"],
        "pending_second_admin"
    );
    assert_eq!(account_status(&env, owner.id).await, "active");
    // The same admin again: still pending.
    let again = act(&env, &first_session, item, &body).await;
    assert_eq!(again.status(), StatusCode::ACCEPTED);
    assert_eq!(account_status(&env, owner.id).await, "active");
    // A different admin confirms.
    let confirmed = act(&env, &second_session, item, &body).await;
    assert_eq!(confirmed.status(), StatusCode::OK);
    assert_eq!(account_status(&env, owner.id).await, "banned");
    let (actor, second_actor): (Uuid, Option<Uuid>) = sqlx::query_as(
        "SELECT actor_id, second_actor_id FROM moderation.moderation_actions WHERE queue_item_id = $1",
    )
    .bind(item)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!((actor, second_actor), (first.account, Some(second.account)));
}

#[tokio::test]
async fn actions_need_staff_csrf_and_a_reason() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    let session = signed_in_staff(&env, &env.router, &moderator).await;
    let (owner, _, item) = reported_link(&env).await;
    // No CSRF token.
    let request = Request::post(format!("/v1/admin/reports/{item}/actions"))
        .header(header::COOKIE, &session.cookie)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"action": "dismiss", "reason": "x"}).to_string(),
        ))
        .unwrap();
    assert_eq!(
        send(&env.router, request).await.status(),
        StatusCode::FORBIDDEN
    );
    // An app bearer token.
    let request = Request::post(format!("/v1/admin/reports/{item}/actions"))
        .header("authorization", format!("Bearer {}", owner.token))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"action": "dismiss", "reason": "x"}).to_string(),
        ))
        .unwrap();
    assert_eq!(
        send(&env.router, request).await.status(),
        StatusCode::UNAUTHORIZED
    );
    // Blank reason; unknown item.
    let blank = act(
        &env,
        &session,
        item,
        &json!({"action": "dismiss", "reason": "  "}),
    )
    .await;
    assert_eq!(blank.status(), StatusCode::BAD_REQUEST);
    let unknown = act(
        &env,
        &session,
        Uuid::now_v7(),
        &json!({"action": "dismiss", "reason": "x"}),
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
}
