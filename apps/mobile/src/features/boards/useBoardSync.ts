import { useQueryClient } from '@tanstack/react-query';
import { useCallback, useEffect, useRef, useState } from 'react';
import { AppState } from 'react-native';

import { t, type MessageKey } from '../../i18n';
import { useToast } from '../../ui';
import type { BoardRepository, SyncOutcome } from './boardRepository';

export const boardQueryKey = (id: string) => ['boards', id] as const;

/** Edits are pushed this long after the last one, so a burst of moves goes out as one batch. */
export const SYNC_DEBOUNCE_MS = 1500;

export type BoardSyncStatus = 'idle' | 'syncing' | 'offline' | 'removed';

/** What each refused op means to the person who made it (server codes of the board ops endpoint). */
const REJECTION_MESSAGES: Readonly<Record<string, MessageKey>> = {
  PIN_NOT_YOURS: 'boards.sync.rejected.notYours',
  PIN_LIMIT_REACHED: 'boards.sync.rejected.limit',
  CARD_HELD_FOR_REVIEW: 'boards.sync.rejected.held',
  NOT_FOUND: 'boards.sync.rejected.notFound',
};

/**
 * Keeps one board in sync (T-105, AC-18.3, AC-19.4): on open, when the app comes back to the foreground, on refresh,
 * and shortly after my edits. Conflicts and refused edits are explained once per sync; a board I was removed from
 * says so. Repositories without a server (tests, the lab) never sync.
 */
export function useBoardSync(repository: BoardRepository, boardId: string) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [status, setStatus] = useState<BoardSyncStatus>('idle');
  const [pending, setPending] = useState(() => repository.pendingCount?.(boardId) ?? 0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mounted = useRef(true);

  const sync = useCallback(async () => {
    if (!repository.sync) return;
    setStatus('syncing');
    const outcome: SyncOutcome = await repository.sync(boardId);
    if (!mounted.current) return;
    const board = await repository.get(boardId);
    queryClient.setQueryData(boardQueryKey(boardId), board);
    setPending(repository.pendingCount?.(boardId) ?? 0);
    switch (outcome.kind) {
      case 'synced': {
        setStatus('idle');
        if (outcome.conflicts > 0) toast.show({ message: t('boards.sync.conflict') });
        for (const code of new Set(outcome.rejected)) {
          toast.show({
            message: t(REJECTION_MESSAGES[code] ?? 'boards.sync.rejected.other'),
            variant: 'error',
          });
        }
        return;
      }
      case 'offline':
        setStatus('offline');
        return;
      case 'limit':
        setStatus('removed');
        toast.show({ message: t('boards.error.limit'), variant: 'error' });
        return;
      case 'removed':
        setStatus('removed');
        return;
      case 'signed_out':
        setStatus('idle');
        return;
    }
  }, [repository, boardId, queryClient, toast]);

  const schedule = useCallback(() => {
    setPending(repository.pendingCount?.(boardId) ?? 0);
    if (!repository.sync) return;
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => void sync(), SYNC_DEBOUNCE_MS);
  }, [repository, boardId, sync]);

  useEffect(() => {
    mounted.current = true;
    // On the next tick, not inside the effect body (the sync updates state right away).
    const opening = setTimeout(() => void sync(), 0);
    const subscription = AppState.addEventListener('change', (state) => {
      if (state === 'active') void sync();
    });
    return () => {
      mounted.current = false;
      clearTimeout(opening);
      subscription.remove();
      if (timer.current) clearTimeout(timer.current);
    };
  }, [sync]);

  return { status, pending, sync, schedule };
}
