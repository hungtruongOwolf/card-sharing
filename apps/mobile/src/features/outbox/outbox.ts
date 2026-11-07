import { AuthError } from '../auth/authClient';
import type { PreparedCard } from '../cards/prepare';
import { isRetryable, type CardsApi } from '../sharing/cardsApi';
import { runSend, type SendJob } from '../sharing/sendCard';
import { reactionEntryId, type OutboxEntry, type OutboxStore } from './outboxStore';

/** First retry delay; doubles per attempt up to {@link MAX_BACKOFF_MS}. */
export const BASE_BACKOFF_MS = 5_000;
export const MAX_BACKOFF_MS = 15 * 60_000;
/** prd-mvp / T-78: after 24 h of failed attempts an entry stops and shows "Not sent - retry or delete". */
export const GIVE_UP_AFTER_MS = 24 * 3_600_000;

export const backoffMs = (attempts: number): number =>
  Math.min(MAX_BACKOFF_MS, BASE_BACKOFF_MS * 2 ** Math.max(0, attempts - 1));

export interface OutboxDeps {
  store: OutboxStore;
  api: CardsApi;
  /** Bakes the draft `cardId` again for a send that still needs its uploads (the bake is byte-stable). */
  prepareSend: (cardId: string, keepOriginal: boolean) => Promise<PreparedCard>;
  /** Drops a sent card from the drafts tray (best effort). */
  removeDraft: (cardId: string) => Promise<void>;
  now?: () => number;
}

export interface Outbox {
  list(): Promise<OutboxEntry[]>;
  /** Queues a send that could not go out now; resolves false when the outbox cannot hold it. */
  enqueueSend(job: SendJob): Promise<boolean>;
  /** Queues my reaction (null removes it); a newer reaction on the same card replaces a queued one. */
  enqueueReaction(cardId: string, reaction: string | null): Promise<boolean>;
  /** Removes an entry (cancel a pending send, delete a failed one). */
  cancel(id: string): Promise<void>;
  /** Makes a failed entry pending again with a fresh 24 h window, then processes the outbox. */
  retry(id: string): Promise<void>;
  /** Runs every entry that is due, one at a time. Concurrent calls share one run. */
  process(): Promise<void>;
  /**
   * Runs every pending entry now, ignoring its backoff (the device just came back online). Failed entries stay
   * failed; an attempt that fails again backs off as usual. Waits for a run in progress first.
   */
  processNow(): Promise<void>;
  /** Epoch milliseconds of the next pending attempt, or null. */
  nextDueAt(): Promise<number | null>;
  clear(): Promise<void>;
  /** Called after every change (queued, sent, failed, removed). */
  subscribe(listener: () => void): () => void;
}

export function createOutbox({
  store,
  api,
  prepareSend,
  removeDraft,
  now = Date.now,
}: OutboxDeps): Outbox {
  const listeners = new Set<() => void>();
  const changed = () => {
    for (const listener of listeners) listener();
  };
  let running: Promise<void> | null = null;

  const put = async (entry: OutboxEntry): Promise<boolean> => {
    try {
      await store.put(entry);
      changed();
      return true;
    } catch {
      return false;
    }
  };

  /** One attempt; resolves with the entry to keep (or null when it is done). */
  async function attempt(entry: OutboxEntry): Promise<OutboxEntry | null> {
    // The job as it progresses: uploads that finish during a failed attempt are kept for the next one.
    let job = entry.kind === 'send' ? entry.job : null;
    try {
      if (entry.kind === 'send') {
        const sent = await runSend(entry.job, {
          api,
          prepare: (keep) => prepareSend(entry.job.cardId, keep),
          onProgress: (next) => {
            job = next;
          },
        });
        removeDraft(sent.job.cardId).catch(() => undefined);
        return null;
      }
      if (entry.reaction) await api.setReaction(entry.cardId, entry.reaction);
      else await api.clearReaction(entry.cardId);
      return null;
    } catch (error) {
      // Signed out meanwhile: leave the entry untouched (sign-out clears the outbox anyway).
      if (error instanceof AuthError && error.kind !== 'unavailable') return entry;
      const at = now();
      const attempts = entry.attempts + 1;
      const keep = isRetryable(error) && at - entry.createdAt < GIVE_UP_AFTER_MS;
      const next = {
        attempts,
        nextAttemptAt: at + backoffMs(attempts),
        state: keep ? ('pending' as const) : ('failed' as const),
      };
      return entry.kind === 'send'
        ? { ...entry, ...next, job: job ?? entry.job }
        : { ...entry, ...next };
    }
  }

  async function runDue(ignoreBackoff = false): Promise<void> {
    for (const entry of await store.list()) {
      if (entry.state !== 'pending') continue;
      if (!ignoreBackoff && entry.nextAttemptAt > now()) continue;
      const next = await attempt(entry);
      // The user may have cancelled it while the attempt ran: do not bring it back.
      const stillQueued = (await store.list()).some((e) => e.id === entry.id);
      if (!stillQueued) continue;
      if (next) await store.put(next).catch(() => undefined);
      else await store.remove(entry.id);
      changed();
    }
  }

  return {
    list: () => store.list(),
    enqueueSend: (job) =>
      put({
        kind: 'send',
        id: job.clientCardId,
        job,
        createdAt: now(),
        attempts: 0,
        nextAttemptAt: now() + BASE_BACKOFF_MS,
        state: 'pending',
      }),
    enqueueReaction: (cardId, reaction) =>
      put({
        kind: 'reaction',
        id: reactionEntryId(cardId),
        cardId,
        reaction,
        createdAt: now(),
        attempts: 0,
        nextAttemptAt: now() + BASE_BACKOFF_MS,
        state: 'pending',
      }),
    async cancel(id) {
      await store.remove(id);
      changed();
    },
    async retry(id) {
      const entry = (await store.list()).find((e) => e.id === id);
      if (!entry) return;
      await put({
        ...entry,
        state: 'pending',
        attempts: 0,
        createdAt: now(),
        nextAttemptAt: now(),
      });
      await this.process();
    },
    process() {
      running ??= runDue().finally(() => {
        running = null;
      });
      return running;
    },
    async processNow() {
      await running?.catch(() => undefined);
      running ??= runDue(true).finally(() => {
        running = null;
      });
      return running;
    },
    async nextDueAt() {
      const due = (await store.list())
        .filter((entry) => entry.state === 'pending')
        .map((entry) => entry.nextAttemptAt);
      return due.length > 0 ? Math.min(...due) : null;
    },
    async clear() {
      await store.clear();
      changed();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}
