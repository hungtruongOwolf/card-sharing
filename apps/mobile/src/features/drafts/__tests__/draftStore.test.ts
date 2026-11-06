import * as SecureStore from 'expo-secure-store';

import { registry, type EffectEntry } from '@card-sharing/effects-registry';

import { secureStoreMock } from '../../../test/secureStoreMock';
import { newLocalCard, type LocalCard } from '../../cards/localCard';
import type { CapturedPhoto } from '../../capture';
import { styleChoice, type CardDraft } from '../../editor/editorState';
import {
  createDraftStore,
  DRAFT_INDEX_KEY,
  draftKey,
  DraftStoreError,
  MAX_DRAFTS,
  memoryKeyValueStore,
  migrateChoice,
  parseLocalCard,
  secureKeyValueStore,
  utf8Length,
} from '../draftStore';
import { isLowStorage, LOW_STORAGE_BYTES, staleDrafts } from '../housekeeping';

// eslint-disable-next-line @typescript-eslint/no-require-imports -- jest.mock factories must use require()
jest.mock('expo-secure-store', () => require('../../../test/secureStoreMock').secureStoreMock);

function entry(id: string): EffectEntry {
  const found = registry.get(id);
  if (!found) throw new Error(`missing ${id}`);
  return found;
}

const photo: CapturedPhoto = {
  uri: 'file:///cache/ImagePicker/a.jpg',
  width: 300,
  height: 400,
  origin: 'camera',
};
const draft: CardDraft = {
  cut: styleChoice(entry('cut.diecut')),
  finish: styleChoice(entry('fx.matte')),
  effects: [],
  wholePhoto: false,
  caption: 'zoomies',
};

let seq = 0;
const card = (updatedAt = '2026-11-05T10:00:00.000Z', overrides: Partial<LocalCard> = {}) => ({
  ...newLocalCard(photo, draft, new Date(updatedAt), `id-${++seq}`),
  ...overrides,
});

describe('draft store', () => {
  it('saves, lists newest first, reads and removes drafts', async () => {
    const store = createDraftStore(memoryKeyValueStore());
    const older = card('2026-11-01T10:00:00.000Z');
    const newer = card('2026-11-04T10:00:00.000Z');
    await store.save(older);
    await store.save(newer);
    expect((await store.list()).map((c) => c.id)).toEqual([newer.id, older.id]);
    expect(await store.get(older.id)).toEqual(older);
    await store.remove(older.id);
    expect((await store.list()).map((c) => c.id)).toEqual([newer.id]);
    expect(await store.get(older.id)).toBeNull();
  });

  it('survives an app restart (a new store over the same storage)', async () => {
    const kv = memoryKeyValueStore();
    const saved = card();
    await createDraftStore(kv).save(saved);
    expect(await createDraftStore(kv).list()).toEqual([saved]);
  });

  it('updates a draft in place instead of duplicating it', async () => {
    const store = createDraftStore(memoryKeyValueStore());
    const first = card();
    await store.save(first);
    await store.save({ ...first, draft: { ...draft, caption: 'edited' } });
    const all = await store.list();
    expect(all).toHaveLength(1);
    expect(all[0]!.draft.caption).toBe('edited');
  });

  it('refuses new drafts when full and oversized records', async () => {
    const kv = memoryKeyValueStore();
    kv.items.set(
      DRAFT_INDEX_KEY,
      JSON.stringify(Array.from({ length: MAX_DRAFTS }, (_, i) => `full-${i}`)),
    );
    const store = createDraftStore(kv);
    await expect(store.save(card())).rejects.toEqual(new DraftStoreError('full'));
    const huge = card(undefined, { draft: { ...draft, caption: 'x'.repeat(3000) } });
    await expect(createDraftStore(memoryKeyValueStore()).save(huge)).rejects.toMatchObject({
      reason: 'tooLarge',
    });
    await expect(
      createDraftStore(memoryKeyValueStore()).save(card(undefined, { id: '../evil' })),
    ).rejects.toMatchObject({ reason: 'invalid' });
  });

  it('skips corrupt or tampered records instead of crashing the tray', async () => {
    const kv = memoryKeyValueStore();
    const good = card();
    await createDraftStore(kv).save(good);
    kv.items.set(DRAFT_INDEX_KEY, JSON.stringify([good.id, 'bad-1', 'bad-2', good.id, 42]));
    kv.items.set(draftKey('bad-1'), '{not json');
    kv.items.set(
      draftKey('bad-2'),
      JSON.stringify({
        ...card(),
        id: 'bad-2',
        photo: { ...photo, uri: 'https://evil.example/x' },
      }),
    );
    expect(await createDraftStore(kv).list()).toEqual([good]);
  });

  it('keeps drafts in the Keychain/Keystore, this device only, never in a readable backup', async () => {
    secureStoreMock.__reset();
    const store = createDraftStore(secureKeyValueStore());
    await store.save(card());
    expect(secureStoreMock.__calls.length).toBeGreaterThan(0);
    for (const call of secureStoreMock.__calls) {
      expect(call.options).toEqual({
        keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY,
      });
    }
  });
});

describe('parseLocalCard', () => {
  const valid = card();

  it('round-trips a stored card and sanitizes the caption', () => {
    expect(parseLocalCard(JSON.stringify(valid))).toEqual(valid);
    const bidi = { ...valid, draft: { ...valid.draft, caption: 'hi‮there' } };
    expect(parseLocalCard(JSON.stringify(bidi))?.draft.caption).toBe('hithere');
  });

  it.each([
    ['another schema', { ...valid, schema: 2 }],
    ['a remote photo', { ...valid, photo: { ...photo, uri: 'https://x.example/a.jpg' } }],
    ['a broken date', { ...valid, updatedAt: 'yesterday' }],
    ['object params', { ...valid, draft: { ...draft, cut: { ...draft.cut, params: { a: {} } } } }],
    ['a missing caption', { ...valid, draft: { ...draft, caption: undefined } }],
    ['effects that are not a list', { ...valid, draft: { ...draft, effects: { id: 'fx.holo' } } }],
    ['a broken effect', { ...valid, draft: { ...draft, effects: [{ id: 'fx.holo' }] } }],
    [
      'more effects than a card holds',
      { ...valid, draft: { ...draft, effects: Array.from({ length: 9 }, () => draft.cut) } },
    ],
  ])('rejects %s', (_label, value) => {
    expect(parseLocalCard(JSON.stringify(value))).toBeNull();
  });

  it('keeps stacked effects and reads drafts saved before effects existed (T-114)', () => {
    const stacked = { ...valid, draft: { ...draft, effects: [draft.cut, draft.cut] } };
    expect(parseLocalCard(JSON.stringify(stacked))?.draft.effects).toEqual([draft.cut, draft.cut]);
    // JSON.stringify drops the undefined field, like a record saved by an older build.
    const older = { ...draft, effects: undefined };
    expect(parseLocalCard(JSON.stringify({ ...valid, draft: older }))?.draft.effects).toEqual([]);
  });

  it('migrates retired registry ids (T-124 ledger)', () => {
    const retired = { 'fx.old': 'fx.matte', 'fx.gone': null };
    expect(migrateChoice(draft.cut, registry, retired)).toBe(draft.cut);
    expect(
      migrateChoice({ id: 'fx.old', version: 3, params: { a: 1 } }, registry, retired),
    ).toEqual({
      id: 'fx.matte',
      version: 1,
      params: {},
    });
    expect(migrateChoice({ id: 'fx.gone', version: 1, params: {} }, registry, retired)).toBeNull();
  });

  it('rejects a record stored under another id', () => {
    expect(parseLocalCard(JSON.stringify(valid), 'someone-else')).toBeNull();
  });
});

describe('housekeeping', () => {
  it('offers drafts older than 30 days for cleanup', () => {
    const now = new Date('2026-12-10T00:00:00.000Z');
    const fresh = card('2026-11-20T00:00:00.000Z');
    const old = card('2026-11-01T00:00:00.000Z');
    expect(staleDrafts([fresh, old], now)).toEqual([old]);
  });

  it('warns below 500 MB free and stays quiet when unknown', () => {
    expect(isLowStorage(LOW_STORAGE_BYTES - 1)).toBe(true);
    expect(isLowStorage(LOW_STORAGE_BYTES)).toBe(false);
    expect(isLowStorage(null)).toBe(false);
  });

  it('counts UTF-8 bytes', () => {
    expect(utf8Length('aé€\u{1F431}')).toBe(1 + 2 + 3 + 4);
  });
});
