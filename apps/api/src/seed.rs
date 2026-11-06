//! Local demo data (T-3): `api seed` creates two adult test users, one minor, friendships, a few
//! delivered cards and a shared board, then prints a fresh session per user.
//!
//! - Development and test only: `main` refuses to seed when `API_ENV` is staging or production.
//! - Idempotent: users are found by their fixed seed identity, friendships use `ON CONFLICT`, and
//!   cards and the board are created only when the first adult has none yet. Running it twice
//!   leaves the same data (plus one more session per user).
//! - Cards and boards go through the real HTTP routes in-process (same validation, idempotency
//!   and authorization as the app), and media through the real pipeline with the configured
//!   content gates (the fake adapters locally). Accounts, ages and profiles use the identity
//!   service directly; friendships are written like the test fixtures do.
//! - The schema comes from the production migrations only; nothing here creates tables.

use axum::Router;
use axum::body::Body;
use http::{Request, StatusCode};
use identity::{AccountStatus, AgeOutcome, AgeSubmission, IssuedSession, Provider};
use identity::{RegionSignals, VerifiedIdentity};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
use media::ProcessSettings;
use serde_json::{Value, json};
use time::{Date, Duration};
use tower::ServiceExt;
use uuid::Uuid;

use crate::state::AppState;

/// One seeded person.
#[derive(Debug, Clone, Copy)]
pub struct Persona {
    /// Short key, also the suffix of the seed identity (`seed.<key>`).
    pub key: &'static str,
    pub display_name: &'static str,
    pub handle: &'static str,
    /// Age in whole years on the day of seeding.
    pub age: i32,
}

/// Two adults and one minor (the minor's age is above every region's minimum in
/// config/policy/regions.yaml and below adulthood).
pub const PERSONAS: [Persona; 3] = [
    Persona {
        key: "alice",
        display_name: "Alice (demo)",
        handle: "alice.demo",
        age: 31,
    },
    Persona {
        key: "bob",
        display_name: "Bob (demo)",
        handle: "bob.demo",
        age: 29,
    },
    Persona {
        key: "mia",
        display_name: "Mia (demo, minor)",
        handle: "mia.demo",
        age: 15,
    },
];

/// Seed identities use a provider subject no real Apple account can have (Apple subjects are
/// opaque `000000.<hex>.0000` strings), so nobody can sign in as a seed user through Apple.
pub fn seed_subject(key: &str) -> String {
    format!("seed.{key}")
}

/// A seeded user with a fresh session.
pub struct SeededUser {
    pub persona: Persona,
    pub id: Uuid,
    pub session: IssuedSession,
}

/// What `run` created or found.
pub struct SeedReport {
    pub users: Vec<SeededUser>,
    /// Cards created by this run (0 when they already existed).
    pub cards_created: usize,
    /// The shared demo board (created now or found).
    pub board_id: Uuid,
}

/// Seeds the demo data. `gates` must pass clean images (the fake adapters locally), otherwise
/// the cards are never delivered and this fails.
pub async fn run(
    router: &Router,
    state: &AppState,
    gates: &ProcessSettings,
) -> anyhow::Result<SeedReport> {
    let mut users = Vec::with_capacity(PERSONAS.len());
    for persona in PERSONAS {
        let id = ensure_user(state, persona).await?;
        let session = state.identity.start_session(id, None).await?;
        users.push(SeededUser {
            persona,
            id,
            session,
        });
    }
    let [alice, bob, mia] = [&users[0], &users[1], &users[2]];
    make_friends(state, alice.id, bob.id).await?;
    make_friends(state, alice.id, mia.id).await?;

    let existing: i64 = sqlx::query_scalar("SELECT count(*) FROM cards.cards WHERE owner_id = $1")
        .bind(alice.id)
        .fetch_one(state.identity.pool())
        .await?;
    let mut cards_created = 0;
    if existing == 0 {
        let first = send_card(
            router,
            state,
            gates,
            alice,
            &[bob.id, mia.id],
            "First zoomies of the day",
            CardArt::new([255, 183, 77], [244, 81, 30]),
        )
        .await?;
        let second = send_card(
            router,
            state,
            gates,
            alice,
            &[bob.id],
            "Breakfast thief",
            CardArt::new([129, 212, 250], [57, 73, 171]),
        )
        .await?;
        let reply = send_card(
            router,
            state,
            gates,
            bob,
            &[alice.id],
            "Nap time",
            CardArt::new([197, 225, 165], [56, 142, 60]),
        )
        .await?;
        cards_created = 3;
        let board_id = shared_board(router, alice, bob, &[first, second, reply]).await?;
        return Ok(SeedReport {
            users,
            cards_created,
            board_id,
        });
    }
    let board_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM boards.boards WHERE owner_id = $1 AND deleted_at IS NULL
         ORDER BY created_at LIMIT 1",
    )
    .bind(alice.id)
    .fetch_optional(state.identity.pool())
    .await?;
    let board_id = board_id.ok_or_else(|| {
        anyhow::anyhow!(
            "the first seed user has cards but no board: reset the database and seed again"
        )
    })?;
    Ok(SeedReport {
        users,
        cards_created,
        board_id,
    })
}

/// Finds or creates the persona's account and walks it through the age gate and profile step.
async fn ensure_user(state: &AppState, persona: Persona) -> anyhow::Result<Uuid> {
    let identity = VerifiedIdentity {
        provider: Provider::Apple,
        subject: seed_subject(persona.key),
        email: None,
    };
    let (id, _) = state
        .identity
        .find_or_create_account(&identity, &RegionSignals::default())
        .await?;
    let account = state
        .identity
        .account(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("seed account {} vanished", persona.key))?;
    if account.status == AccountStatus::PendingAge {
        let us = "US".parse().ok();
        let submission = AgeSubmission {
            birth_date: birth_date(state, persona.age)?,
            store_country: us,
            declared_country: us,
            ip_country: None,
        };
        let outcome = state
            .identity
            .submit_age(id, None, &submission, &state.policy, &state.field_cipher)
            .await?;
        if outcome == AgeOutcome::Refused {
            anyhow::bail!(
                "the age gate refused seed user {} (check config/policy/regions.yaml)",
                persona.key
            );
        }
    }
    let rules = &state.profile.rules;
    let filter = &state.profile.text_filter;
    state
        .identity
        .set_display_name(id, persona.display_name, filter, rules)
        .await
        .map_err(|e| anyhow::anyhow!("display name for {}: {e}", persona.key))?;
    state
        .identity
        .claim_handle(id, persona.handle, filter, rules)
        .await
        .map_err(|e| anyhow::anyhow!("handle for {}: {e}", persona.key))?;
    Ok(id)
}

/// A birth date that makes the persona exactly `age` today (birthday 30 days ago).
fn birth_date(state: &AppState, age: i32) -> anyhow::Result<Date> {
    let day = state.identity.clock().now().date() - Duration::days(30);
    let birth = day
        .replace_year(day.year() - age)
        .or_else(|_| (day - Duration::days(1)).replace_year(day.year() - age))?;
    Ok(birth)
}

async fn make_friends(state: &AppState, a: Uuid, b: Uuid) -> anyhow::Result<()> {
    let (low, high) = if a < b { (a, b) } else { (b, a) };
    sqlx::query(
        "INSERT INTO social.friendships (user_a, user_b, created_at) VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(low)
    .bind(high)
    .bind(state.identity.clock().now())
    .execute(state.identity.pool())
    .await?;
    Ok(())
}

/// Colors of a generated card image: a vertical gradient with a round "subject".
#[derive(Debug, Clone, Copy)]
struct CardArt {
    background: [u8; 3],
    subject: [u8; 3],
}

impl CardArt {
    const fn new(background: [u8; 3], subject: [u8; 3]) -> Self {
        Self {
            background,
            subject,
        }
    }

    const WIDTH: u32 = 600;
    const HEIGHT: u32 = 800;

    /// Inside the round subject; its size follows the subject color, so every card's mask differs.
    fn inside(self, x: u32, y: u32) -> bool {
        let r = 150.0 + f32::from(self.subject[0] % 64);
        let (cx, cy) = (300.0_f32, 420.0_f32);
        let (dx, dy) = (x as f32 - cx, y as f32 - cy);
        dx * dx + dy * dy <= r * r
    }

    fn render(self) -> anyhow::Result<Vec<u8>> {
        let [r, g, b] = self.background;
        let [sr, sg, sb] = self.subject;
        let image = RgbaImage::from_fn(Self::WIDTH, Self::HEIGHT, |x, y| {
            if self.inside(x, y) {
                return Rgba([sr, sg, sb, 255]);
            }
            // Darken towards the bottom; the top-left pixel never matches a fake-gate marker.
            let shade = 1.0 - (y as f32 / Self::HEIGHT as f32) * 0.35;
            let tone = |c: u8| (f32::from(c) * shade) as u8;
            Rgba([tone(r), tone(g), tone(b), 255])
        });
        encode(&image)
    }

    fn mask(self) -> anyhow::Result<Vec<u8>> {
        let image = RgbaImage::from_fn(Self::WIDTH, Self::HEIGHT, |x, y| {
            let v = if self.inside(x, y) { 255 } else { 0 };
            Rgba([v, v, v, 255])
        });
        encode(&image)
    }
}

fn encode(image: &RgbaImage) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    PngEncoder::new(&mut out).write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}

/// Uploads, processes and sends one card, then runs delivery; returns the card id.
async fn send_card(
    router: &Router,
    state: &AppState,
    gates: &ProcessSettings,
    sender: &SeededUser,
    recipients: &[Uuid],
    caption: &str,
    art: CardArt,
) -> anyhow::Result<Uuid> {
    let render = upload(router, sender, "card_render", art.render()?).await?;
    let mask = upload(router, sender, "mask", art.mask()?).await?;
    for (id, clean) in [render, mask] {
        if clean {
            continue;
        }
        state.media.process_one(id, gates).await.map_err(|e| {
            anyhow::anyhow!(
                "media processing failed ({e:?}); set API_CSAM_MATCHER=fake and API_NUDITY_CLASSIFIER=fake"
            )
        })?;
    }
    let (render, mask) = (render.0, mask.0);
    let client_card_id = Uuid::now_v7();
    let body = json!({
        "client_card_id": client_card_id,
        "render_media_id": render,
        "mask_media_id": mask,
        "style": { "cut": { "id": "cut.diecut", "version": 1, "params": { "borderWidthPct": 2.5 } } },
        "caption": caption,
        "audience": { "mode": "subset", "recipient_ids": recipients },
    });
    let response = call(
        router,
        "POST",
        "/v1/cards",
        sender,
        Some(&body),
        Some(client_card_id),
    )
    .await?;
    let card = id_at(
        &response,
        StatusCode::ACCEPTED,
        "/card/id",
        "POST /v1/cards",
    )?;
    let outcome = state.cards.evaluate(&state.media, card).await?;
    if outcome != cards::Evaluation::Delivered {
        anyhow::bail!("seed card was not delivered: {outcome:?}");
    }
    Ok(card)
}

async fn upload(
    router: &Router,
    user: &SeededUser,
    kind: &str,
    bytes: Vec<u8>,
) -> anyhow::Result<(Uuid, bool)> {
    let request = Request::post(format!("/v1/media?kind={kind}"))
        .header("authorization", bearer(user))
        .header("content-type", "image/png")
        .header("content-length", bytes.len())
        .body(Body::from(bytes))?;
    let response = send(router, request).await?;
    // 200 = the same bytes were uploaded before (dedupe): the existing media is reused.
    let status = if response.status == StatusCode::OK {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    let id = id_at(&response, status, "/media/id", "POST /v1/media")?;
    let clean = response
        .body
        .pointer("/media/state")
        .and_then(Value::as_str)
        == Some("clean");
    Ok((id, clean))
}

/// Alice's board with all three cards pinned, shared with Bob (invited and accepted).
async fn shared_board(
    router: &Router,
    owner: &SeededUser,
    member: &SeededUser,
    cards: &[Uuid],
) -> anyhow::Result<Uuid> {
    let board = Uuid::now_v7();
    let response = call(
        router,
        "POST",
        "/v1/boards",
        owner,
        Some(&json!({ "id": board, "title": "Weekend scrapbook" })),
        None,
    )
    .await?;
    expect(&response, StatusCode::CREATED, "POST /v1/boards")?;
    let ops: Vec<Value> = cards
        .iter()
        .zip([(0.25, 0.22, -8), (0.7, 0.3, 6), (0.45, 0.68, -3)])
        .zip(["a0", "a1", "a2"])
        .map(|((card, (x, y, rotation)), z_key)| {
            json!({ "type": "pin.add", "op_id": Uuid::now_v7(), "pin": {
                "id": Uuid::now_v7(), "card_id": card, "x": x, "y": y, "scale": 1,
                "rotation": rotation, "z_key": z_key } })
        })
        .collect();
    let response = call(
        router,
        "POST",
        &format!("/v1/boards/{board}/ops"),
        owner,
        Some(&json!({ "ops": ops })),
        None,
    )
    .await?;
    expect(&response, StatusCode::OK, "POST /v1/boards/{id}/ops")?;
    let response = call(
        router,
        "POST",
        &format!("/v1/boards/{board}/invitations"),
        owner,
        Some(&json!({ "user_id": member.id })),
        Some(Uuid::now_v7()),
    )
    .await?;
    let invitation = id_at(
        &response,
        StatusCode::CREATED,
        "/invitation/id",
        "POST /v1/boards/{id}/invitations",
    )?;
    let response = call(
        router,
        "POST",
        &format!("/v1/board-invitations/{invitation}/accept"),
        member,
        None,
        None,
    )
    .await?;
    expect(
        &response,
        StatusCode::OK,
        "POST /v1/board-invitations/{id}/accept",
    )?;
    Ok(board)
}

struct Answer {
    status: StatusCode,
    body: Value,
}

fn bearer(user: &SeededUser) -> String {
    format!("Bearer {}", user.session.access_token.expose())
}

async fn call(
    router: &Router,
    method: &str,
    path: &str,
    user: &SeededUser,
    body: Option<&Value>,
    idempotency_key: Option<Uuid>,
) -> anyhow::Result<Answer> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", bearer(user));
    if let Some(key) = idempotency_key {
        builder = builder.header("idempotency-key", key.to_string());
    }
    let request = match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))?,
        None => builder.body(Body::empty())?,
    };
    send(router, request).await
}

async fn send(router: &Router, request: Request<Body>) -> anyhow::Result<Answer> {
    let response = router.clone().oneshot(request).await?;
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await?;
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    Ok(Answer { status, body })
}

fn expect(answer: &Answer, status: StatusCode, what: &str) -> anyhow::Result<()> {
    if answer.status == status {
        return Ok(());
    }
    anyhow::bail!(
        "{what} answered {} (expected {status}): {}",
        answer.status,
        answer.body
    )
}

fn id_at(answer: &Answer, status: StatusCode, pointer: &str, what: &str) -> anyhow::Result<Uuid> {
    expect(answer, status, what)?;
    answer
        .body
        .pointer(pointer)
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(|| anyhow::anyhow!("{what}: no id at {pointer} in {}", answer.body))
}
