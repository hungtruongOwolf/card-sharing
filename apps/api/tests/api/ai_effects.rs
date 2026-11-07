//! AI effects on the fake provider (sprint 9, genai-effects.md): availability by region/age policy,
//! explicit consent, server-side credits (reserve, refund, replay), owner-only jobs, and the job
//! run through the moderation hooks and the media gates. Every assertion is about accounts this
//! test created (the database is shared with concurrent tests).

use crate::common::*;
use crate::media_gate::fake_gates;
use crate::media_process::uploaded_id;
use crate::social_support::*;
use axum::body::Body;
use genai::{FAKE_INPUT_REFUSAL, FakeProvider, Gates, RunOutcome, UnconfiguredProvider};
use http::{Request, Response, StatusCode};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
use serde_json::{Value, json};
use uuid::Uuid;

/// A PNG whose top-left pixel is `corner` (the fake gates read it) and the rest a gradient.
fn png_with_corner(corner: [u8; 3]) -> Vec<u8> {
    let salt = Uuid::now_v7();
    let salt = salt.as_bytes();
    let pixels = RgbaImage::from_fn(64, 48, |x, y| {
        if x == 0 && y == 0 {
            Rgba([corner[0], corner[1], corner[2], 255])
        } else {
            let i = ((x + y) as usize) % salt.len();
            Rgba([salt[i], (x * 3 % 256) as u8, (y * 5 % 256) as u8, 255])
        }
    });
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(pixels.as_raw(), 64, 48, ExtendedColorType::Rgba8)
        .unwrap();
    out
}

/// A clean card render of `user` with the given top-left pixel.
async fn clean_render(env: &TestEnv, user: &User, corner: [u8; 3]) -> Uuid {
    let id = uploaded_id(env, user, "card_render", png_with_corner(corner)).await;
    env.state
        .media
        .process_one(id, &fake_gates())
        .await
        .unwrap();
    id
}

fn job_body(template: &str, input: Uuid) -> Value {
    let version = 1;
    json!({
        "client_job_id": Uuid::now_v7(),
        "template_id": template,
        "template_version": version,
        "input_media_id": input,
    })
}

async fn post_job(env: &TestEnv, user: &User, body: &Value) -> Response<Body> {
    let key = body["client_job_id"].as_str().unwrap_or("").to_owned();
    let request = Request::post("/v1/ai-effects/jobs")
        .header("authorization", format!("Bearer {}", user.token))
        .header("content-type", "application/json")
        .header("idempotency-key", key)
        .body(Body::from(body.to_string()))
        .unwrap();
    send(&env.router, request).await
}

async fn consent(env: &TestEnv, user: &User) {
    let response = call(
        env,
        "PUT",
        "/v1/ai-effects/consent",
        user,
        Some(&json!({ "version": 1 })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

async fn overview(env: &TestEnv, user: &User) -> Value {
    let response = call(env, "GET", "/v1/ai-effects", user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn submitted(env: &TestEnv, user: &User, template: &str, input: Uuid) -> Uuid {
    let response = post_job(env, user, &job_body(template, input)).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = json_body(response).await;
    assert_eq!(body["job"]["status"], "queued");
    Uuid::parse_str(body["job"]["id"].as_str().unwrap()).unwrap()
}

async fn job(env: &TestEnv, user: &User, id: Uuid) -> Value {
    let response = call(env, "GET", &format!("/v1/ai-effects/jobs/{id}"), user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["job"].clone()
}

#[tokio::test]
async fn adults_see_templates_and_credits_minors_see_nothing() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let adult = adult(&env).await;
    let body = overview(&env, &adult).await;
    assert_eq!(body["available"], true);
    assert_eq!(
        body["consent"],
        json!({ "required_version": 1, "granted": false })
    );
    assert_eq!(body["credits"], 5);
    let ids: Vec<&str> = body["templates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"ai.watercolor"));
    assert!(
        body["templates"][0].get("prompt").is_none(),
        "prompts stay on the server"
    );
    assert_eq!(body["ai_label"]["visible_badge"], true);

    // Under ai_effects.min_age (18, Gemini terms): hidden, and submits are refused even with
    // consent and a clean upload.
    let minor = minor(&env).await;
    let body = overview(&env, &minor).await;
    assert_eq!(body["available"], false);
    assert_eq!(body["templates"], json!([]));
    consent(&env, &minor).await;
    let input = clean_render(&env, &minor, [10, 20, 30]).await;
    let response = post_job(&env, &minor, &job_body("ai.watercolor", input)).await;
    assert_problem(response, StatusCode::FORBIDDEN, "POLICY_DISALLOWED").await;
}

#[tokio::test]
async fn consent_is_required_and_must_be_the_current_version() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let input = clean_render(&env, &user, [10, 20, 30]).await;
    let response = post_job(&env, &user, &job_body("ai.watercolor", input)).await;
    assert_problem(response, StatusCode::FORBIDDEN, "CONSENT_REQUIRED").await;

    let stale = call(
        &env,
        "PUT",
        "/v1/ai-effects/consent",
        &user,
        Some(&json!({ "version": 7 })),
    )
    .await;
    assert_problem(stale, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;

    consent(&env, &user).await;
    assert_eq!(overview(&env, &user).await["consent"]["granted"], true);
    submitted(&env, &user, "ai.watercolor", input).await;

    // Withdrawn: the next submit is refused again.
    let response = call(&env, "DELETE", "/v1/ai-effects/consent", &user, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = post_job(&env, &user, &job_body("ai.watercolor", input)).await;
    assert_problem(response, StatusCode::FORBIDDEN, "CONSENT_REQUIRED").await;
}

#[tokio::test]
async fn submit_reserves_credits_once_and_refuses_when_they_run_out() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    consent(&env, &user).await;
    let input = clean_render(&env, &user, [10, 20, 30]).await;

    let body = job_body("ai.vintage_photo", input); // costs 2
    let first = json_body(post_job(&env, &user, &body).await).await;
    let replay = post_job(&env, &user, &body).await;
    assert_eq!(replay.status(), StatusCode::ACCEPTED);
    assert_eq!(json_body(replay).await["job"]["id"], first["job"]["id"]);
    assert_eq!(
        overview(&env, &user).await["credits"],
        3,
        "a replay is not charged"
    );

    submitted(&env, &user, "ai.vintage_photo", input).await;
    assert_eq!(overview(&env, &user).await["credits"], 1);
    let response = post_job(&env, &user, &job_body("ai.vintage_photo", input)).await;
    assert_problem(
        response,
        StatusCode::PAYMENT_REQUIRED,
        "INSUFFICIENT_CREDITS",
    )
    .await;
    submitted(&env, &user, "ai.watercolor", input).await; // costs 1
    assert_eq!(overview(&env, &user).await["credits"], 0);
}

#[tokio::test]
async fn inputs_must_be_my_clean_uploads_and_templates_curated() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let other = adult(&env).await;
    consent(&env, &user).await;
    let theirs = clean_render(&env, &other, [10, 20, 30]).await;
    let response = post_job(&env, &user, &job_body("ai.watercolor", theirs)).await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    let quarantined = uploaded_id(&env, &user, "card_render", png_with_corner([1, 2, 3])).await;
    let response = post_job(&env, &user, &job_body("ai.watercolor", quarantined)).await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;

    let mine = clean_render(&env, &user, [10, 20, 30]).await;
    for template in ["ai.face_swap", "ai.watercolor\u{0}"] {
        let response = post_job(&env, &user, &job_body(template, mine)).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    let mut body = job_body("ai.watercolor", mine);
    body["prompt"] = json!("anything goes");
    let response = post_job(&env, &user, &body).await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    assert_eq!(
        overview(&env, &user).await["credits"],
        5,
        "nothing was charged"
    );
}

#[tokio::test]
async fn a_job_runs_on_the_fake_provider_and_its_output_enters_the_media_gates() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    consent(&env, &user).await;
    let input = clean_render(&env, &user, [10, 20, 30]).await;
    let id = submitted(&env, &user, "ai.neon_night", input).await;
    let outcome = env
        .state
        .genai
        .run(id, &env.state.media, &FakeProvider, &Gates::fake(), false)
        .await
        .unwrap();
    assert_eq!(outcome, RunOutcome::Succeeded);
    let view = job(&env, &user, id).await;
    assert_eq!(view["status"], "succeeded");
    assert_eq!(view["ai_generated"], true);
    assert_eq!(view["c2pa_recorded"], true);
    assert_eq!(view["refunded"], false);
    let output = Uuid::parse_str(view["output_media_id"].as_str().unwrap()).unwrap();
    // The output is the caller's card render, waiting for the CSAM and nudity gates like any upload.
    let record = env
        .state
        .media
        .owned(user.id, output)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.kind, media::MediaKind::CardRender);
    assert_eq!(record.state, media::MediaState::Uploaded);
    assert_eq!(overview(&env, &user).await["credits"], 4);

    // Running again is a no-op.
    let again = env
        .state
        .genai
        .run(id, &env.state.media, &FakeProvider, &Gates::fake(), false)
        .await
        .unwrap();
    assert_eq!(again, RunOutcome::Done);
}

#[tokio::test]
async fn refusals_and_failures_refund_the_reserved_credits() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    consent(&env, &user).await;

    // Input moderation (fake: green top-left pixel).
    let flagged = clean_render(&env, &user, FAKE_INPUT_REFUSAL).await;
    let id = submitted(&env, &user, "ai.watercolor", flagged).await;
    let outcome = env
        .state
        .genai
        .run(id, &env.state.media, &FakeProvider, &Gates::fake(), false)
        .await
        .unwrap();
    assert_eq!(outcome, RunOutcome::Refused);
    let view = job(&env, &user, id).await;
    assert_eq!(view["status"], "refused");
    assert_eq!(view["outcome"], "input_moderation");
    assert_eq!(view["refunded"], true);
    assert_eq!(view["output_media_id"], Value::Null);

    // Output moderation: `invert` turns [0, 127, 255] into the fake output marker.
    let input = clean_render(&env, &user, [0, 127, 255]).await;
    let id = submitted(&env, &user, "ai.neon_night", input).await;
    let outcome = env
        .state
        .genai
        .run(id, &env.state.media, &FakeProvider, &Gates::fake(), false)
        .await
        .unwrap();
    assert_eq!(outcome, RunOutcome::Refused);
    assert_eq!(job(&env, &user, id).await["outcome"], "output_moderation");

    // Provider outage: retried until the last attempt, then failed and refunded.
    let input = clean_render(&env, &user, [10, 20, 30]).await;
    let id = submitted(&env, &user, "ai.vintage_photo", input).await;
    let gates = Gates::fake();
    let genai = &env.state.genai;
    let media = &env.state.media;
    let outcome = genai
        .run(id, media, &UnconfiguredProvider, &gates, false)
        .await;
    assert_eq!(outcome.unwrap(), RunOutcome::Retry);
    assert_eq!(job(&env, &user, id).await["status"], "queued");
    assert_eq!(overview(&env, &user).await["credits"], 3);
    let outcome = genai
        .run(id, media, &UnconfiguredProvider, &gates, true)
        .await;
    assert_eq!(outcome.unwrap(), RunOutcome::Failed);
    let view = job(&env, &user, id).await;
    assert_eq!(view["outcome"], "provider_failed");
    assert_eq!(view["refunded"], true);
    assert_eq!(
        overview(&env, &user).await["credits"],
        5,
        "every refusal was refunded"
    );
}

#[tokio::test]
async fn jobs_are_visible_to_their_owner_only() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let other = adult(&env).await;
    consent(&env, &owner).await;
    let input = clean_render(&env, &owner, [10, 20, 30]).await;
    let id = submitted(&env, &owner, "ai.watercolor", input).await;
    let theirs = call(
        &env,
        "GET",
        &format!("/v1/ai-effects/jobs/{id}"),
        &other,
        None,
    )
    .await;
    let missing = call(
        &env,
        "GET",
        &format!("/v1/ai-effects/jobs/{}", Uuid::now_v7()),
        &other,
        None,
    )
    .await;
    let theirs = assert_problem(theirs, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    let missing = assert_problem(missing, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    assert_eq!(theirs["title"], missing["title"]);
    assert_eq!(theirs["detail"], missing["detail"]);
}

#[tokio::test]
async fn without_a_provider_nothing_is_offered_or_accepted() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let state = env
        .state
        .clone()
        .with_genai(genai::GenAiSettings::shipped(), false);
    let router = api::build(api::openapi::router(), state, &test_config());
    let user = adult(&env).await;
    let response = send(
        &router,
        json_request("GET", "/v1/ai-effects", Some(&user.token), None),
    )
    .await;
    assert_eq!(json_body(response).await["available"], false);
    let input = clean_render(&env, &user, [10, 20, 30]).await;
    let body = job_body("ai.watercolor", input);
    let request = Request::post("/v1/ai-effects/jobs")
        .header("authorization", format!("Bearer {}", user.token))
        .header("content-type", "application/json")
        .header("idempotency-key", body["client_job_id"].as_str().unwrap())
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = send(&router, request).await;
    assert_problem(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "TEMPORARILY_UNAVAILABLE",
    )
    .await;
}
