//! T-73: the nudity classifier runs after the hash gate on card renders and avatars. A flag
//! holds the media in quarantine (never signed); an outage keeps it `scanning`; the model and
//! score are stored for audit.

use crate::common::*;
use crate::media_gate::fake_gates;
use crate::media_process::{real_png, real_png_with, uploaded_id};
use crate::social_support::*;
use image::Rgba;
use media::Bucket;
use media::safety::{FAKE_CLASSIFIER_DOWN_MARKER, FAKE_NUDITY_MARKER};
use platform::jobs::JobError;
use uuid::Uuid;

async fn verdict(env: &TestEnv, id: Uuid) -> (String, String, Option<f32>, Option<String>) {
    sqlx::query_as(
        "SELECT state, bucket, classifier_score, classifier_model FROM media.media_objects WHERE id = $1",
    )
    .bind(id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn flagged_renders_and_avatars_are_held_in_quarantine() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let clean = env.media_store.count(Bucket::Clean);
    for kind in ["card_render", "avatar"] {
        let id = uploaded_id(
            &env,
            &user,
            kind,
            real_png_with(48, 48, Rgba(FAKE_NUDITY_MARKER)),
        )
        .await;
        env.state
            .media
            .process_one(id, &fake_gates())
            .await
            .unwrap();
        let (state, bucket, score, model) = verdict(&env, id).await;
        assert_eq!(
            (state.as_str(), bucket.as_str()),
            ("held", "quarantine"),
            "{kind}"
        );
        assert!(score.unwrap() > 0.9);
        assert_eq!(model.as_deref(), Some("fake-nudity-1"));
        let response = call(&env, "GET", &format!("/v1/media/{id}"), &user, None).await;
        let body = json_body(response).await;
        assert_eq!(body["media"]["state"], "held");
        assert!(!body.to_string().contains("url"));
    }
    assert_eq!(
        env.media_store.count(Bucket::Clean),
        clean,
        "nothing held is servable"
    );
}

#[tokio::test]
async fn masks_and_originals_are_not_classified() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    for marker in [FAKE_NUDITY_MARKER, FAKE_CLASSIFIER_DOWN_MARKER] {
        for kind in ["mask", "original"] {
            let id = uploaded_id(&env, &user, kind, real_png_with(32, 32, Rgba(marker))).await;
            env.state
                .media
                .process_one(id, &fake_gates())
                .await
                .unwrap();
            let (state, _, score, _) = verdict(&env, id).await;
            assert_eq!(state, "clean", "{kind}");
            assert_eq!(score, None);
        }
    }
}

#[tokio::test]
async fn classifier_outage_keeps_the_render_scanning() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let id = uploaded_id(
        &env,
        &user,
        "card_render",
        real_png_with(32, 32, Rgba(FAKE_CLASSIFIER_DOWN_MARKER)),
    )
    .await;
    let result = env.state.media.process_one(id, &fake_gates()).await;
    assert!(matches!(result, Err(JobError::Retry(_))), "{result:?}");
    assert_eq!(verdict(&env, id).await.0, "scanning");
}

#[tokio::test]
async fn clean_renders_keep_the_classifier_verdict_for_audit() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let id = uploaded_id(&env, &user, "card_render", real_png(32, 32)).await;
    env.state
        .media
        .process_one(id, &fake_gates())
        .await
        .unwrap();
    let (state, bucket, score, model) = verdict(&env, id).await;
    assert_eq!((state.as_str(), bucket.as_str()), ("clean", "clean"));
    assert!(score.unwrap() < 0.1);
    assert_eq!(model.as_deref(), Some("fake-nudity-1"));
}
