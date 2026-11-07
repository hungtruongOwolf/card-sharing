//! Real-time stream hub ([[ADR-0010-realtime-delivery]]): one Postgres listener per API instance
//! on `platform::realtime::CHANNEL`, and a map `account id -> broadcast::Sender` so an event
//! reaches only the open streams of its addressee. Events carry ids only; `GET /v1/events`
//! (`routes::events`) turns them into SSE frames and the app refetches through the normal read
//! routes, which authorize every id again.
//!
//! Limits: at most `per_account_max` open streams per account and `instance_max` per instance
//! (`API_EVENTS_MAX_PER_ACCOUNT`, `API_EVENTS_MAX_PER_INSTANCE`). When the listener loses its
//! connection, every open stream gets `resync` (events may have been missed) once it is back.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use platform::realtime::{CHANNEL, RealtimeEvent};
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

/// Events buffered per account before a slow stream is told to `resync`.
const ACCOUNT_BUFFER: usize = 32;

/// Stream limits and timings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealtimeSettings {
    /// Open streams per account (devices and tabs). Default 5.
    pub per_account_max: usize,
    /// Open streams per API instance. Default 2000.
    pub instance_max: usize,
    /// SSE comment sent this often so proxies keep the stream open. 25 s.
    pub heartbeat: Duration,
    /// Every stream is closed after this long (the client reconnects). 30 min.
    pub max_stream: Duration,
    /// The stream's bearer token is re-checked this often; a revoked session or an expired
    /// access token ends the stream (the reconnect then gets 401 or refreshes). 30 s.
    pub session_recheck: Duration,
}

impl Default for RealtimeSettings {
    fn default() -> Self {
        Self {
            per_account_max: 5,
            instance_max: 2000,
            heartbeat: Duration::from_secs(25),
            max_stream: Duration::from_secs(30 * 60),
            session_recheck: Duration::from_secs(30),
        }
    }
}

/// What a stream receives from the hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Event(RealtimeEvent),
    /// Events may have been missed: refetch.
    Resync,
}

/// Why a stream could not open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The account already has `per_account_max` open streams.
    AccountCap,
    /// The instance already holds `instance_max` streams, or it is shutting down.
    Busy,
}

struct Entry {
    sender: broadcast::Sender<Signal>,
    streams: usize,
}

#[derive(Default)]
struct Inner {
    accounts: HashMap<Uuid, Entry>,
    total: usize,
}

/// The per-instance hub. Cheap to share (`Arc`).
pub struct RealtimeHub {
    settings: RealtimeSettings,
    inner: Mutex<Inner>,
    shutdown: watch::Sender<bool>,
    /// `true` while the listener is subscribed to the channel.
    listening: watch::Sender<bool>,
}

/// One open stream: its receiver, the shutdown signal, and a guard that frees its slot on drop.
pub struct Subscription {
    pub signals: broadcast::Receiver<Signal>,
    pub shutdown: watch::Receiver<bool>,
    _slot: Slot,
}

struct Slot {
    hub: Arc<RealtimeHub>,
    account: Uuid,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.hub.release(self.account);
    }
}

impl RealtimeHub {
    pub fn new(settings: RealtimeSettings) -> Arc<Self> {
        Arc::new(Self {
            settings,
            inner: Mutex::new(Inner::default()),
            shutdown: watch::channel(false).0,
            listening: watch::channel(false).0,
        })
    }

    pub fn settings(&self) -> RealtimeSettings {
        self.settings
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while holding the lock leaves plain counters behind: keep going.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Opens a stream slot for `account`, within the per-account and per-instance caps.
    pub fn subscribe(self: &Arc<Self>, account: Uuid) -> Result<Subscription, Refusal> {
        if *self.shutdown.borrow() {
            return Err(Refusal::Busy);
        }
        let mut inner = self.lock();
        if inner.total >= self.settings.instance_max {
            return Err(Refusal::Busy);
        }
        let entry = inner.accounts.entry(account).or_insert_with(|| Entry {
            sender: broadcast::channel(ACCOUNT_BUFFER).0,
            streams: 0,
        });
        if entry.streams >= self.settings.per_account_max {
            return Err(Refusal::AccountCap);
        }
        entry.streams += 1;
        let signals = entry.sender.subscribe();
        inner.total += 1;
        drop(inner);
        Ok(Subscription {
            signals,
            shutdown: self.shutdown.subscribe(),
            _slot: Slot {
                hub: Arc::clone(self),
                account,
            },
        })
    }

    fn release(&self, account: Uuid) {
        let mut inner = self.lock();
        inner.total = inner.total.saturating_sub(1);
        if let Some(entry) = inner.accounts.get_mut(&account) {
            entry.streams = entry.streams.saturating_sub(1);
            if entry.streams == 0 {
                inner.accounts.remove(&account);
            }
        }
    }

    /// Forwards `event` to its addressee's open streams on this instance (if any).
    pub fn dispatch(&self, event: RealtimeEvent) {
        let inner = self.lock();
        if let Some(entry) = inner.accounts.get(&event.to) {
            // No receiver left is fine: the stream is closing.
            let _ = entry.sender.send(Signal::Event(event));
        }
    }

    /// Tells every open stream to refetch (the listener may have missed events).
    pub fn resync_all(&self) {
        let inner = self.lock();
        for entry in inner.accounts.values() {
            let _ = entry.sender.send(Signal::Resync);
        }
    }

    /// Open streams on this instance.
    pub fn open_streams(&self) -> usize {
        self.lock().total
    }

    /// Ends every stream (graceful shutdown) and refuses new ones.
    pub fn close_all(&self) {
        let _ = self.shutdown.send(true);
    }

    fn closed(&self) -> bool {
        *self.shutdown.borrow()
    }

    /// Waits (at most 10 s) until the listener is subscribed: events committed before that are
    /// not seen by this instance (tests; readiness).
    pub async fn wait_listening(&self) {
        let mut listening = self.listening.subscribe();
        let _ = tokio::time::timeout(Duration::from_secs(10), listening.wait_for(|on| *on)).await;
    }
}

/// The instance's listener: forwards NOTIFYs on [`CHANNEL`] to the hub until it closes.
/// Reconnects after a failure and then asks every stream to resync.
pub async fn listen(pool: PgPool, hub: Arc<RealtimeHub>) {
    let mut shutdown = hub.shutdown.subscribe();
    let mut lost = false;
    while !hub.closed() {
        let mut listener = match PgListener::connect_with(&pool).await {
            Ok(listener) => listener,
            Err(err) => {
                tracing::warn!(error = %err, "realtime listener cannot connect; retrying");
                if wait(&mut shutdown, Duration::from_secs(5)).await {
                    return;
                }
                lost = true;
                continue;
            }
        };
        if let Err(err) = listener.listen(CHANNEL).await {
            tracing::warn!(error = %err, "realtime LISTEN failed; retrying");
            if wait(&mut shutdown, Duration::from_secs(5)).await {
                return;
            }
            lost = true;
            continue;
        }
        let _ = hub.listening.send(true);
        if std::mem::take(&mut lost) {
            hub.resync_all();
        }
        loop {
            tokio::select! {
                _ = shutdown.changed() => return,
                notification = listener.recv() => match notification {
                    Ok(notification) => {
                        if let Some(event) = RealtimeEvent::parse(notification.payload()) {
                            hub.dispatch(event);
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "realtime listener lost; reconnecting");
                        let _ = hub.listening.send(false);
                        lost = true;
                        break;
                    }
                },
            }
        }
    }
}

/// Sleeps `duration` unless shutdown comes first (`true` = shut down).
async fn wait(shutdown: &mut watch::Receiver<bool>, duration: Duration) -> bool {
    tokio::select! {
        _ = shutdown.changed() => true,
        () = tokio::time::sleep(duration) => *shutdown.borrow(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform::realtime::EventKind;

    fn hub(per_account_max: usize, instance_max: usize) -> Arc<RealtimeHub> {
        RealtimeHub::new(RealtimeSettings {
            per_account_max,
            instance_max,
            ..RealtimeSettings::default()
        })
    }

    fn event(to: Uuid) -> RealtimeEvent {
        RealtimeEvent {
            to,
            kind: EventKind::CardDelivered,
            card_id: Uuid::now_v7(),
            reply_to: None,
        }
    }

    #[tokio::test]
    async fn events_reach_only_their_addressee() {
        let hub = hub(5, 10);
        let (alice, bob) = (Uuid::now_v7(), Uuid::now_v7());
        let mut a = hub.subscribe(alice).unwrap_or_else(|_| panic!("open"));
        let mut b = hub.subscribe(bob).unwrap_or_else(|_| panic!("open"));
        let sent = event(alice);
        hub.dispatch(sent);
        assert_eq!(a.signals.try_recv().ok(), Some(Signal::Event(sent)));
        assert!(b.signals.try_recv().is_err());
        hub.resync_all();
        assert_eq!(b.signals.try_recv().ok(), Some(Signal::Resync));
    }

    #[tokio::test]
    async fn caps_hold_and_slots_free_on_drop() {
        let hub = hub(2, 3);
        let (alice, bob) = (Uuid::now_v7(), Uuid::now_v7());
        let first = hub.subscribe(alice).ok();
        let second = hub.subscribe(alice).ok();
        assert!(first.is_some() && second.is_some());
        assert_eq!(hub.subscribe(alice).err(), Some(Refusal::AccountCap));
        let third = hub.subscribe(bob).ok();
        assert!(third.is_some());
        assert_eq!(hub.subscribe(bob).err(), Some(Refusal::Busy));
        drop(first);
        assert_eq!(hub.open_streams(), 2);
        assert!(hub.subscribe(alice).is_ok());
        hub.close_all();
        assert_eq!(hub.subscribe(Uuid::now_v7()).err(), Some(Refusal::Busy));
    }
}
