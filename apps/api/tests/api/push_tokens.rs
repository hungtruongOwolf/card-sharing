//! T-85 (US-31, US-16, SR-15, SR-30): a session registers push tokens only for its own device;
//! a token moves between accounts instead of being shared; logout and suspension stop pushes; the
//! token is stored encrypted and never echoed.

use crate::common::*;
use crate::social_support::*;
use http::StatusCode;
use identity::{DeviceInfo, Platform, RevokeReason};
use notifications::TokenKind;
use serde_json::{Value, json};
use uuid::Uuid;

/// `user` signed in again on a new device; returns the device session and its ids.
pub async fn on_device(env: &TestEnv, user: &User) -> (User, Uuid, Uuid) {
    let device = Uuid::now_v7();
    let session = env
        .identity
        .start_session(
            user.id,
            Some(&DeviceInfo {
                id: device,
                platform: Platform::Ios,
                app_version: "1.0.0".to_owned(),
            }),
        )
        .await
        .unwrap();
    let on_device = User {
        token: session.access_token.expose().to_owned(),
        ..user.clone()
    };
    (on_device, device, session.session_id)
}

pub fn apns_token() -> String {
    format!("{}{}", Uuid::now_v7().simple(), Uuid::now_v7().simple())
}

pub async fn put(env: &TestEnv, user: &User, device: Uuid, body: &Value) -> StatusCode {
    call(
        env,
        "PUT",
        &format!("/v1/devices/{device}/push-tokens"),
        user,
        Some(body),
    )
    .await
    .status()
}

async fn live(env: &TestEnv, account: Uuid) -> Vec<String> {
    env.state
        .push_tokens
        .live_targets(account, TokenKind::Alert)
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.token.expose().to_owned())
        .collect()
}

#[tokio::test]
async fn register_replace_and_remove_my_device_token() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (me, device, _) = on_device(&env, &adult(&env).await).await;
    let first = apns_token();
    let body = json!({ "provider": "apns", "kind": "alert", "token": first });
    assert_eq!(put(&env, &me, device, &body).await, StatusCode::NO_CONTENT);
    // Idempotent, and a widget token is separate.
    assert_eq!(put(&env, &me, device, &body).await, StatusCode::NO_CONTENT);
    let widget = json!({ "provider": "apns", "kind": "widget", "token": apns_token() });
    assert_eq!(
        put(&env, &me, device, &widget).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(live(&env, me.id).await, vec![first.clone()]);

    // A new token replaces the old one of the same kind.
    let second = apns_token();
    let body = json!({ "provider": "apns", "kind": "alert", "token": second });
    assert_eq!(put(&env, &me, device, &body).await, StatusCode::NO_CONTENT);
    assert_eq!(live(&env, me.id).await, vec![second.clone()]);

    // Stored encrypted: the token does not appear in the row.
    let stored: Vec<u8> = sqlx::query_scalar(
        "SELECT token_enc FROM notifications.push_tokens WHERE account_id = $1 AND kind = 'alert'",
    )
    .bind(me.id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert!(!String::from_utf8_lossy(&stored).contains(&second));

    let response = call(
        &env,
        "DELETE",
        &format!("/v1/devices/{device}/push-tokens"),
        &me,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM notifications.push_tokens WHERE account_id = $1")
            .bind(me.id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn a_token_moved_to_another_account_stops_reaching_the_first() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (a, a_device, _) = on_device(&env, &adult(&env).await).await;
    let (b, b_device, _) = on_device(&env, &adult(&env).await).await;
    let token = apns_token();
    let body = json!({ "provider": "apns", "kind": "alert", "token": token });
    assert_eq!(put(&env, &a, a_device, &body).await, StatusCode::NO_CONTENT);
    assert_eq!(live(&env, a.id).await, vec![token.clone()]);

    assert_eq!(put(&env, &b, b_device, &body).await, StatusCode::NO_CONTENT);
    assert!(live(&env, a.id).await.is_empty());
    assert_eq!(live(&env, b.id).await, vec![token]);
}

#[tokio::test]
async fn logout_and_suspension_stop_pushes_and_prune_removes_the_rows() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let (me, device, session) = on_device(&env, &user).await;
    let (other, other_device, _) = on_device(&env, &user).await;
    let body = json!({ "provider": "apns", "kind": "alert", "token": apns_token() });
    assert_eq!(put(&env, &me, device, &body).await, StatusCode::NO_CONTENT);
    let fcm = "fcm-token:APA91b_x".to_owned();
    let body = json!({ "provider": "fcm", "kind": "alert", "token": fcm });
    assert_eq!(
        put(&env, &other, other_device, &body).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(live(&env, user.id).await.len(), 2);

    // Logging out one device stops only that device's pushes.
    env.identity
        .revoke_session(session, RevokeReason::Logout)
        .await
        .unwrap();
    assert_eq!(live(&env, user.id).await, vec![fcm]);
    assert!(env.state.push_tokens.prune().await.unwrap() >= 1);
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM notifications.push_tokens WHERE account_id = $1")
            .bind(user.id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(left, 1);

    // A suspended account receives nothing.
    sqlx::query("UPDATE identity.accounts SET status = 'suspended' WHERE id = $1")
        .bind(user.id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    assert!(live(&env, user.id).await.is_empty());
}

#[tokio::test]
async fn another_device_or_account_is_404_and_bad_bodies_are_400() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (me, device, _) = on_device(&env, &adult(&env).await).await;
    let (_, victim_device, _) = on_device(&env, &adult(&env).await).await;
    let good = json!({ "provider": "apns", "kind": "alert", "token": apns_token() });

    // The victim's device id, a random one, a malformed one: the same 404, nothing stored.
    let missing = call(
        &env,
        "PUT",
        &format!("/v1/devices/{}/push-tokens", Uuid::now_v7()),
        &me,
        Some(&good),
    )
    .await;
    let missing = assert_problem(missing, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    for path in [
        format!("/v1/devices/{victim_device}/push-tokens"),
        "/v1/devices/not-a-uuid/push-tokens".to_owned(),
    ] {
        let response = call(&env, "PUT", &path, &me, Some(&good)).await;
        let body = assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
        assert_eq!(body["detail"], missing["detail"]);
        let response = call(&env, "DELETE", &path, &me, None).await;
        assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    }
    // A session without a device cannot register at all.
    let plain = adult(&env).await;
    let response = call(
        &env,
        "PUT",
        &format!("/v1/devices/{device}/push-tokens"),
        &plain,
        Some(&good),
    )
    .await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;

    for body in [
        json!({ "provider": "gcm", "kind": "alert", "token": apns_token() }),
        json!({ "provider": "apns", "kind": "silent", "token": apns_token() }),
        json!({ "provider": "apns", "kind": "alert", "token": "not hex" }),
        json!({ "provider": "fcm", "kind": "alert", "token": "has space" }),
        json!({ "provider": "apns", "kind": "alert" }),
        json!({ "provider": "apns", "kind": "alert", "token": apns_token(), "user_id": me.id }),
    ] {
        let response = call(
            &env,
            "PUT",
            &format!("/v1/devices/{device}/push-tokens"),
            &me,
            Some(&body),
        )
        .await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    assert!(live(&env, me.id).await.is_empty());
}
