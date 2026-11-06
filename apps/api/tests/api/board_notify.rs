//! Board change notifications (T-109, AC-19.4, SR-15): `board.invite` to the invitee (never for a
//! silent drop), `board.joined` to the owner, and one debounced silent `board.updated` per board
//! and window carrying the board id and version only.

use crate::board_invitations::{accept, invited};
use crate::board_ops::{add_card, apply, setup};
use crate::common::*;
use crate::social_support::*;
use http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

async fn events(env: &TestEnv, topic: &str, board: Uuid) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT payload FROM platform.outbox_events
         WHERE topic = $1 AND payload->>'board_id' = $2 ORDER BY id",
    )
    .bind(topic)
    .bind(board.to_string())
    .fetch_all(env.identity.pool())
    .await
    .unwrap()
}

async fn changed_jobs(env: &TestEnv, board: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM platform.jobs WHERE kind = 'boards.changed' AND payload->>'board_id' = $1",
    )
    .bind(board.to_string())
    .fetch_one(env.identity.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn invitations_and_joins_notify_the_right_people_and_drops_stay_silent() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    let invitation = invited(&env, &s.owner, s.board, &s.friend).await;
    let created = events(&env, boards::INVITATION_CREATED, s.board).await;
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["invitee_id"], s.friend.id.to_string());
    assert_eq!(created[0]["invitation_id"], invitation.to_string());
    let recipients = notifications::recipients_of(boards::INVITATION_CREATED, &created[0]);
    assert_eq!(recipients.len(), 1);
    assert_eq!(recipients[0].account_id, s.friend.id);

    assert_eq!(
        accept(&env, &s.friend, invitation).await.status(),
        StatusCode::OK
    );
    let joined = events(&env, boards::MEMBER_JOINED, s.board).await;
    assert_eq!(joined.len(), 1);
    let recipients = notifications::recipients_of(boards::MEMBER_JOINED, &joined[0]);
    assert_eq!(recipients[0].account_id, s.owner.id);

    // A silently dropped invitation sends nothing to anyone.
    let blocked = adult(&env).await;
    make_friends(&env, s.owner.id, blocked.id).await;
    block(&env, s.friend.id, blocked.id).await;
    invited(&env, &s.owner, s.board, &blocked).await;
    assert_eq!(
        events(&env, boards::INVITATION_CREATED, s.board)
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn changes_coalesce_into_one_silent_push_with_id_and_version_only() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let s = setup(&env).await;
    for _ in 0..3 {
        apply(
            &env,
            &s.owner,
            s.board,
            json!([add_card(Uuid::now_v7(), s.card)]),
        )
        .await;
    }
    // Same debounce window (the test clock does not move): one job.
    assert_eq!(changed_jobs(&env, s.board).await, 1);
    env.clock
        .advance(time::Duration::seconds(boards::DEBOUNCE_SECS));
    let owner = signed_in_again(&env, &s.owner).await;
    apply(
        &env,
        &owner,
        s.board,
        json!([add_card(Uuid::now_v7(), s.card)]),
    )
    .await;
    assert_eq!(changed_jobs(&env, s.board).await, 2);

    let notifier = boards::ChangeNotifier::new(env.identity.pool().clone(), env.clock.clone());
    assert!(notifier.publish(s.board).await.unwrap());
    let updated = events(&env, boards::BOARD_UPDATED, s.board).await;
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0]["version"], 5);
    assert_eq!(updated[0]["member_ids"], json!([s.owner.id]));
    let recipients = notifications::recipients_of(boards::BOARD_UPDATED, &updated[0]);
    assert_eq!(recipients.len(), 1);
    let fcm = recipients[0].notification.fcm_message();
    assert_eq!(
        fcm["data"],
        json!({ "t": "board.updated", "board_id": s.board.to_string(), "version": "5" })
    );
    assert!(
        fcm["android"].get("notification").is_none(),
        "silent push only"
    );
    assert!(!notifier.publish(Uuid::now_v7()).await.unwrap());
}
