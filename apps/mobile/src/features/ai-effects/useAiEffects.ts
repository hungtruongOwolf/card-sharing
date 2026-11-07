import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { useAuthClient } from '../auth';
import type { BakedImage } from '../cards/bake';
import type { CardDraft } from '../editor/editorState';
import { useCardsApi } from '../sharing/useCardsApi';
import { createAiEffectsApi, type AiEffectsApi, type AiOverview } from './aiEffectsApi';
import {
  runAiEffect,
  type AiPhase,
  type AiRunDeps,
  type AiRunOutcome,
  type AiRunRequest,
} from './runAiEffect';

/** What the editor needs from AI effects; screens and tests depend on this, not on the network. */
export interface AiEffectsController {
  /** Null while loading, after a failed load, or once hidden: no AI entry is shown. */
  readonly overview: AiOverview | null;
  /** Re-reads the overview (credits after a run, consent after a refusal). */
  refresh(): Promise<void>;
  grantConsent(version: number): Promise<void>;
  /** Runs one effect on `draft` as it is now; see {@link runAiEffect}. */
  run(
    request: AiRunRequest,
    draft: CardDraft,
    onPhase: (phase: AiPhase) => void,
  ): Promise<AiRunOutcome>;
  /** The server said policy does not allow AI effects for this account: hide the entry. */
  hide(): void;
}

export interface UseAiEffectsOptions {
  /** Bakes the card render for `draft` (the send bake, so the AI input is exactly what would be sent). */
  render: (draft: CardDraft) => Promise<BakedImage>;
  /** Tests inject an API. */
  api?: AiEffectsApi;
  runDeps?: Partial<AiRunDeps>;
}

/**
 * Loads `GET /v1/ai-effects` once per editor and runs effects over the signed-in client. Whether AI effects are
 * offered (region, age, provider) is the server's `available`, never decided on the device.
 */
export function useAiEffects({
  render,
  api: injected,
  runDeps,
}: UseAiEffectsOptions): AiEffectsController {
  const authClient = useAuthClient();
  const cardsApi = useCardsApi();
  const api = useMemo(
    () => injected ?? createAiEffectsApi(authClient.fetch),
    [injected, authClient],
  );
  const [overview, setOverview] = useState<AiOverview | null>(null);
  const hidden = useRef(false);

  const refresh = useCallback(async () => {
    const next = await api.overview().catch(() => null);
    if (next && !hidden.current) setOverview(next);
  }, [api]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return useMemo<AiEffectsController>(
    () => ({
      overview,
      refresh,
      grantConsent: (version) => api.grantConsent(version),
      run: (request, draft, onPhase) =>
        runAiEffect(request, onPhase, {
          api,
          render: () => render(draft),
          upload: (image) => cardsApi.uploadMedia('card_render', image.contentType, image.bytes),
          ...runDeps,
        }),
      hide: () => {
        hidden.current = true;
        setOverview(null);
      },
    }),
    [overview, refresh, api, cardsApi, render, runDeps],
  );
}
