//! Real-time events (ADR-0010): `GET /v1/events`, a Server-Sent Events stream of ids.
//!
//! - Who: `ses`. Each stream receives only events addressed to the caller's account; the payload
//!   is ids the caller's read routes already authorize (`card_id`, `reply_to`), never captions,
//!   names, media keys or URLs. The app reacts by refetching through those routes.
//! - Frames: `ready` (first frame; refetch now: events are not replayed), `card.delivered
//!   {card_id}`, `card.reply {card_id, reply_to}`, `card.reaction {card_id}` (to the card owner),
//!   `resync` (events may have been missed: refetch), `reconnect {reason}` (the server is closing
//!   the stream: `max_age`, `session` or `shutdown`). A `: hb` comment every 25 s keeps proxies
//!   from closing an idle stream.
//! - Limits: `events.connect` per user; at most 5 open streams per account (429) and a cap per
//!   instance (503 `SERVER_BUSY` + `Retry-After`). Streams close after 30 minutes, and within
//!   30 s once the session is revoked or the access token expires.

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::{self, Stream};
use http::{HeaderMap, HeaderValue, header};
use platform::problem::{ApiError, ProblemDetails};
use platform::realtime::RealtimeEvent;
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{Instant, Interval};
use uuid::Uuid;

use crate::access::{Caller, SessionResolver};
use crate::realtime::{Refusal, Signal, Subscription};
use crate::state::AppState;

pub const TAG: &str = "events";

/// Path of the stream.
pub const EVENTS_PATH: &str = "/v1/events";

struct StreamState {
    subscription: Subscription,
    sessions: Arc<dyn SessionResolver>,
    token: String,
    account: Uuid,
    deadline: Instant,
    recheck: Interval,
    started: bool,
    done: bool,
}

fn frame(event: &RealtimeEvent) -> Event {
    let data = match event.reply_to {
        Some(reply_to) => json!({ "card_id": event.card_id, "reply_to": reply_to }),
        None => json!({ "card_id": event.card_id }),
    };
    Event::default()
        .event(event.kind.as_str())
        .data(data.to_string())
}

fn reconnect(reason: &str) -> Event {
    Event::default()
        .event("reconnect")
        .data(json!({ "reason": reason }).to_string())
}

/// The next frame of a stream, or `None` once it is over.
async fn next_frame(mut s: StreamState) -> Option<(Result<Event, Infallible>, StreamState)> {
    if s.done {
        return None;
    }
    if !s.started {
        s.started = true;
        // The client retries after 3 s at the earliest; its own backoff may wait longer.
        let ready = Event::default()
            .event("ready")
            .data("{}")
            .retry(std::time::Duration::from_secs(3));
        return Some((Ok(ready), s));
    }
    loop {
        tokio::select! {
            () = tokio::time::sleep_until(s.deadline) => {
                s.done = true;
                return Some((Ok(reconnect("max_age")), s));
            }
            _ = s.subscription.shutdown.changed() => {
                s.done = true;
                return Some((Ok(reconnect("shutdown")), s));
            }
            _ = s.recheck.tick() => {
                match s.sessions.resolve(&s.token).await {
                    Ok(principal) if principal.account_id == s.account => {}
                    // Store outage: keep the stream (ids only; the reads behind it still check).
                    Err(crate::access::ResolveError::Unavailable(_)) => {}
                    _ => {
                        s.done = true;
                        return Some((Ok(reconnect("session")), s));
                    }
                }
            }
            signal = s.subscription.signals.recv() => match signal {
                Ok(Signal::Event(event)) => return Some((Ok(frame(&event)), s)),
                Ok(Signal::Resync) | Err(RecvError::Lagged(_)) => {
                    return Some((Ok(Event::default().event("resync").data("{}")), s));
                }
                Err(RecvError::Closed) => return None,
            },
        }
    }
}

fn events_stream(state: StreamState) -> impl Stream<Item = Result<Event, Infallible>> {
    stream::unfold(state, next_frame)
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim().to_owned())
}

/// My real-time events. Auth: ses.
///
/// A `text/event-stream` of ids only (see the frame list in the module docs); refetch through
/// the normal read routes on every frame, and on `ready` after each (re)connect, since events
/// are not replayed. Send the bearer token in `Authorization` (a streaming fetch; the browser
/// `EventSource` cannot send it).
#[utoipa::path(
    get,
    path = "/v1/events",
    tag = TAG,
    operation_id = "streamEvents",
    security(("bearer" = [])),
    responses(
        (status = OK, description = "Server-Sent Events: ready, card.delivered, card.reply, card.reaction, resync, reconnect; data is JSON with ids only.", content_type = "text/event-stream", body = String),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (connection rate, or 5 open streams for this account).", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "SERVER_BUSY: this instance holds its maximum of streams or is shutting down (Retry-After).", body = ProblemDetails),
    )
)]
pub async fn stream_events(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let account = caller.0.account_id;
    let token = bearer(&headers).ok_or(ApiError::Unauthenticated)?;
    let subscription = state
        .realtime
        .subscribe(account)
        .map_err(|refusal| match refusal {
            Refusal::AccountCap => ApiError::RateLimited {
                retry_after_secs: 30,
            },
            Refusal::Busy => ApiError::ServerBusy {
                retry_after_secs: 5,
            },
        })?;
    let settings = state.realtime.settings();
    let now = Instant::now();
    let stream_state = StreamState {
        subscription,
        sessions: Arc::clone(&state.sessions),
        token,
        account,
        deadline: now + settings.max_stream,
        recheck: tokio::time::interval_at(now + settings.session_recheck, settings.session_recheck),
        started: false,
        done: false,
    };
    tracing::info!(
        event = "events.opened",
        open = state.realtime.open_streams()
    );
    let mut response = Sse::new(events_stream(stream_state))
        .keep_alive(KeepAlive::new().interval(settings.heartbeat).text("hb"))
        .into_response();
    // Proxies must not buffer the stream.
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}
