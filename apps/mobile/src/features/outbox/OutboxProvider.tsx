import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { AppState } from 'react-native';

import { netInfoOnlineSource, type OnlineSource } from './connectivity';
import type { Outbox } from './outbox';
import type { OutboxEntry } from './outboxStore';

interface OutboxContextValue {
  outbox: Outbox;
  entries: readonly OutboxEntry[];
}

const OutboxContext = createContext<OutboxContextValue | null>(null);

/**
 * Runs the outbox while the app is signed in: on start, whenever the app comes back to the foreground, when the next
 * backoff is due, and as soon as the device comes back online (#265: every pending entry at once, ignoring its
 * backoff; each retry is the same request under the same idempotency key, so a card is still sent once).
 */
export function OutboxProvider({
  outbox,
  children,
  onlineSource = netInfoOnlineSource,
}: {
  outbox: Outbox;
  children: ReactNode;
  /** Back-online signal (tests inject one). */
  onlineSource?: OnlineSource;
}) {
  const [entries, setEntries] = useState<readonly OutboxEntry[]>([]);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const schedule = async () => {
      const list = await outbox.list().catch(() => []);
      if (cancelled) return;
      setEntries(list);
      if (timer) clearTimeout(timer);
      const due = await outbox.nextDueAt().catch(() => null);
      if (cancelled || due === null) return;
      timer = setTimeout(() => void outbox.process(), Math.max(0, due - Date.now()));
    };
    const unsubscribe = outbox.subscribe(() => void schedule());
    const appState = AppState.addEventListener('change', (state) => {
      if (state === 'active') void outbox.process();
    });
    const offline = onlineSource(() => void outbox.processNow());
    void schedule();
    void outbox.process();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
      unsubscribe();
      appState.remove();
      offline();
    };
  }, [outbox, onlineSource]);

  const value = useMemo(() => ({ outbox, entries }), [outbox, entries]);
  return <OutboxContext.Provider value={value}>{children}</OutboxContext.Provider>;
}

/** The running outbox and its entries, or null outside the provider (isolated screen tests). */
export function useOutbox(): OutboxContextValue | null {
  return useContext(OutboxContext);
}
