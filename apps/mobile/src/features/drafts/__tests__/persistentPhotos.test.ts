import { registry } from '@card-sharing/effects-registry';

import { newLocalCard, withDraft } from '../../cards/localCard';
import type { CapturedPhoto } from '../../capture';
import { styleChoice, type CardDraft } from '../../editor/editorState';
import { createDraftStore, memoryKeyValueStore } from '../draftStore';
import { withPersistentPhotos, type PhotoBlobStore } from '../persistentPhotos';

// Draft records validate the photo URL like the web build does (same-origin `blob:`), plus native files.
jest.mock('../../../platform/localPhoto', () => ({
  isLocalPhotoUri: (uri: string) => uri.startsWith('blob:') || uri.startsWith('file:'),
}));

const ORIGIN = 'http://localhost:8081';
const draft: CardDraft = {
  cut: styleChoice(registry.get('cut.diecut')!),
  finish: null,
  effects: [],
  wholePhoto: false,
  caption: '',
};
const photo = (uri = `blob:${ORIGIN}/aaaa`): CapturedPhoto => ({
  uri,
  width: 300,
  height: 400,
  origin: 'library',
});

function memoryBlobs(): PhotoBlobStore & { items: Map<string, Blob> } {
  const items = new Map<string, Blob>();
  return {
    items,
    get: async (id) => items.get(id) ?? null,
    put: async (id, blob) => {
      items.set(id, blob);
    },
    delete: async (id) => {
      items.delete(id);
    },
  };
}

/** One browser profile: the record store and the blob store survive a "reload"; object URLs do not. */
function profile() {
  const kv = memoryKeyValueStore();
  const blobs = memoryBlobs();
  let urls = 0;
  const live = new Map<string, Blob>();
  const tab = () => {
    const fetchBlob = jest.fn(async (uri: string) => {
      const blob = live.get(uri);
      if (!blob) throw new TypeError('Failed to fetch');
      return blob;
    });
    const revokeUrl = jest.fn((url: string) => live.delete(url));
    const store = withPersistentPhotos(createDraftStore(kv), {
      blobs,
      fetchBlob,
      createUrl: (blob) => {
        const url = `blob:${ORIGIN}/restored-${++urls}`;
        live.set(url, blob);
        return url;
      },
      revokeUrl,
      maxBytes: 1_000,
    });
    return { store, fetchBlob, revokeUrl };
  };
  const reload = () => live.clear();
  const pick = (uri: string, blob: Blob) => live.set(uri, blob);
  return { kv, blobs, tab, reload, pick };
}

const image = (size = 10, type = 'image/jpeg') => new Blob([new Uint8Array(size)], { type });

describe('withPersistentPhotos (web drafts keep their photo)', () => {
  it('keeps the photo bytes once and restores a fresh URL after a reload', async () => {
    const browser = profile();
    const first = browser.tab();
    browser.pick(photo().uri, image());
    const card = newLocalCard(photo(), draft, new Date(), 'draft-1');
    await first.store.save(card);
    await first.store.save(withDraft(card, { ...draft, caption: 'again' }));
    expect(first.fetchBlob).toHaveBeenCalledTimes(1);
    expect(browser.blobs.items.get('draft-1')?.size).toBe(10);
    // Same tab: the original URL is still live and comes back unchanged.
    expect((await first.store.get('draft-1'))?.photo.uri).toBe(photo().uri);

    browser.reload();
    const second = browser.tab();
    const restored = await second.store.get('draft-1');
    expect(restored?.photo.uri).toBe(`blob:${ORIGIN}/restored-1`);
    expect(restored?.draft.caption).toBe('again');
    // Reading again in the same tab reuses the URL; saving it does not copy the bytes again.
    expect((await second.store.list())[0]?.photo.uri).toBe(`blob:${ORIGIN}/restored-1`);
    await second.store.save(restored!);
    expect(second.fetchBlob).not.toHaveBeenCalled();
  });

  it('deletes the bytes and revokes the URL with the draft', async () => {
    const browser = profile();
    browser.pick(photo().uri, image());
    await browser.tab().store.save(newLocalCard(photo(), draft, new Date(), 'draft-1'));
    browser.reload();
    const { store, revokeUrl } = browser.tab();
    await store.get('draft-1');
    await store.remove('draft-1');
    expect(browser.blobs.items.has('draft-1')).toBe(false);
    expect(revokeUrl).toHaveBeenCalledWith(`blob:${ORIGIN}/restored-1`);
    expect(await store.get('draft-1')).toBeNull();
  });

  it('skips photos over the cap or not images, and still saves the draft', async () => {
    const browser = profile();
    const { store } = browser.tab();
    const big = photo(`blob:${ORIGIN}/big`);
    const text = photo(`blob:${ORIGIN}/text`);
    browser.pick(big.uri, image(5_000));
    browser.pick(text.uri, image(10, 'text/html'));
    await store.save(newLocalCard(big, draft, new Date(), 'big'));
    await store.save(newLocalCard(text, draft, new Date(), 'text'));
    expect(browser.blobs.items.size).toBe(0);
    expect(await store.get('big')).not.toBeNull();
  });

  it('never blocks a save when the bytes cannot be kept', async () => {
    const browser = profile();
    const { store } = browser.tab();
    browser.blobs.put = () => Promise.reject(new Error('QuotaExceededError'));
    browser.pick(photo().uri, image());
    await expect(
      store.save(newLocalCard(photo(), draft, new Date(), 'draft-1')),
    ).resolves.toBeUndefined();
    expect(await store.get('draft-1')).not.toBeNull();
  });

  it('leaves native file photos alone', async () => {
    const browser = profile();
    const { store, fetchBlob } = browser.tab();
    const file = { ...photo(), uri: 'file:///data/photo.jpg' };
    await store.save(newLocalCard(file, draft, new Date(), 'native'));
    expect(fetchBlob).not.toHaveBeenCalled();
    expect((await store.get('native'))?.photo.uri).toBe(file.uri);
    expect(browser.blobs.items.size).toBe(0);
  });
});
