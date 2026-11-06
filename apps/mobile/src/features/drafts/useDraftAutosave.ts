import { useCallback, useEffect, useMemo, useRef } from 'react';

import { t } from '../../i18n';
import { useToast } from '../../ui';
import { newLocalCard, withDraft, type LocalCard } from '../cards/localCard';
import type { CapturedPhoto } from '../capture';
import type { CardDraft } from '../editor/editorState';
import { DraftStoreError, type DraftStore } from './draftStore';

/** Quiet time after the last edit before the draft is written (a slider drag is one write). */
export const AUTOSAVE_DELAY_MS = 400;

export interface DraftAutosave {
  /** Debounced save of the shown draft. */
  onDraftChange: (draft: CardDraft) => void;
  /** Saves `draft` now (cancelling a pending save) and resolves with the stored card; throws when it fails. */
  saveNow: (draft: CardDraft) => Promise<LocalCard>;
}

/**
 * Auto-saves the editor's draft to the drafts tray (US-9). A new photo gets one local card id for the whole
 * session; reopening a draft keeps updating that draft. A failed save tells the user once and never blocks editing.
 */
export function useDraftAutosave(
  store: DraftStore,
  photo: CapturedPhoto,
  existing?: LocalCard | null,
): DraftAutosave {
  const toast = useToast();
  const card = useRef<LocalCard | null>(existing ?? null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const latest = useRef<CardDraft | null>(null);
  const warned = useRef(false);

  const flush = useCallback(() => {
    timer.current = null;
    const draft = latest.current;
    if (!draft) return;
    const next = card.current ? withDraft(card.current, draft) : newLocalCard(photo, draft);
    card.current = next;
    store.save(next).catch((error: unknown) => {
      if (warned.current) return;
      warned.current = true;
      const full = error instanceof DraftStoreError && error.reason === 'full';
      toast.show({ message: t(full ? 'drafts.full' : 'drafts.saveFailed'), variant: 'error' });
    });
  }, [photo, store, toast]);

  // Leaving the editor writes the last pending change instead of dropping it.
  useEffect(
    () => () => {
      if (timer.current) {
        clearTimeout(timer.current);
        flush();
      }
    },
    [flush],
  );

  const onDraftChange = useCallback(
    (draft: CardDraft) => {
      latest.current = draft;
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(flush, AUTOSAVE_DELAY_MS);
    },
    [flush],
  );

  const saveNow = useCallback(
    async (draft: CardDraft) => {
      if (timer.current) clearTimeout(timer.current);
      timer.current = null;
      latest.current = draft;
      const next = card.current ? withDraft(card.current, draft) : newLocalCard(photo, draft);
      card.current = next;
      await store.save(next);
      return next;
    },
    [photo, store],
  );

  return useMemo(() => ({ onDraftChange, saveNow }), [onDraftChange, saveNow]);
}
