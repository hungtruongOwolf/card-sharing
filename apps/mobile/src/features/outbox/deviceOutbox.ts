import { registry } from '@card-sharing/effects-registry';

import type { AuthClient } from '../auth';
import { BakeError } from '../cards/bakeError';
import { prepareForSend } from '../cards/prepare';
import { deviceDraftStore } from '../drafts';
import { appMaskSource } from '../editor/appMaskSource';
import { createCardsApi } from '../sharing/cardsApi';
import { createOutbox, type Outbox } from './outbox';
import { createOutboxStore, type OutboxStore } from './outboxStore';

let deviceStore: OutboxStore | null = null;

/** The device's outbox records (SecureStore-backed), created on first use. */
export function deviceOutboxStore(): OutboxStore {
  deviceStore ??= createOutboxStore();
  return deviceStore;
}

/**
 * The app's outbox over the signed-in auth client. A queued send re-bakes its draft from the drafts tray with the
 * same mask source as the editor (the bake is byte-stable, so uploads dedupe on the server).
 */
export function createAppOutbox(authClient: AuthClient): Outbox {
  const maskSource = appMaskSource();
  const drafts = deviceDraftStore();
  return createOutbox({
    store: deviceOutboxStore(),
    api: createCardsApi(authClient.fetch),
    async prepareSend(cardId, keepOriginal) {
      const card = await drafts.get(cardId);
      if (!card) throw new BakeError('draft no longer on this device');
      return prepareForSend(card, keepOriginal, { registry, maskSource });
    },
    removeDraft: (cardId) => drafts.remove(cardId),
  });
}
