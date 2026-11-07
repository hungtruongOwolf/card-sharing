import { registry } from '@card-sharing/effects-registry';

import { lookBasis } from '../../ai-effects/aiLook';
import { newLocalCard } from '../../cards/localCard';
import { styleChoice, type CardDraft } from '../../editor/editorState';
import { newSendJob, runSend } from '../sendCard';

const photo = { uri: 'file:///cache/a.jpg', width: 300, height: 400, origin: 'camera' as const };
const base: CardDraft = {
  cut: styleChoice(registry.get('cut.diecut')!),
  finish: null,
  effects: [],
  wholePhoto: false,
  caption: '',
};
const OUTPUT_ID = '33333333-3333-4333-8333-333333333333';
const ai = {
  jobId: '11111111-1111-4111-8111-111111111111',
  templateId: 'ai.watercolor',
  templateVersion: 1,
  title: 'Watercolor',
  outputMediaId: OUTPUT_ID,
  basis: lookBasis(base),
};

describe('sending a card with an AI look', () => {
  it('sends the AI render as is: no bake, no upload, no original', async () => {
    const card = newLocalCard(photo, { ...base, ai });
    const job = newSendJob(card, registry, { mode: 'all' }, true);
    expect(job.media).toEqual({ render: OUTPUT_ID, mask: null, original: null });
    expect(job.keepOriginal).toBe(false);

    const prepare = jest.fn();
    const api = {
      uploadMedia: jest.fn(),
      createCard: jest
        .fn()
        .mockResolvedValue({ id: 'x', status: 'pending_scan', recipientCount: 1 }),
    };
    await runSend(job, { api: api as never, prepare });
    expect(prepare).not.toHaveBeenCalled();
    expect(api.uploadMedia).not.toHaveBeenCalled();
    expect(api.createCard).toHaveBeenCalledWith(
      expect.objectContaining({ renderMediaId: OUTPUT_ID, originalMediaId: null }),
    );
  });

  it('bakes as usual once the card was edited after the AI look', () => {
    const card = newLocalCard(photo, { ...base, wholePhoto: true, ai });
    const job = newSendJob(card, registry, { mode: 'all' }, true);
    expect(job.media).toBeNull();
    expect(job.keepOriginal).toBe(true);
  });
});
