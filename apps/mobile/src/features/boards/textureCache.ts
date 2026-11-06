import { textureBytes, type TextureLod } from '@card-sharing/canvas-engine';

/** What the cache needs from a decoded image (an `SkImage` in the app, a stub in tests). */
export interface Texture {
  width(): number;
  height(): number;
}

/** GPU texture budget for board pins on a mid-range phone (design-system.md §7). */
export const BOARD_TEXTURE_BUDGET_BYTES = 200 * 1024 * 1024;

export interface TextureRequest {
  /** Stable per image, e.g. the pin's card id (never the signed URL, which changes on every read). */
  key: string;
  url: string;
  lod: TextureLod;
}

export interface TextureCacheDeps<T extends Texture> {
  /** Downloads and decodes `url`, scaled so its long side is at most `lod` px; null on failure. */
  load: (url: string, lod: TextureLod) => Promise<T | null>;
  budgetBytes?: number;
}

export interface TextureCache<T extends Texture> {
  /** The best texture loaded for `key`, any level. */
  get(key: string): T | undefined;
  /**
   * Loads what `requests` need that is not loaded yet at that level (or better) and marks them recently used.
   * Resolves once every load settled; textures over budget are evicted least recently used first, never the
   * ones just requested.
   */
  ensure(requests: readonly TextureRequest[]): Promise<void>;
  /** Called after any texture arrived or was evicted. */
  subscribe(listener: () => void): () => void;
  bytesInUse(): number;
}

interface Slot<T> {
  texture: T;
  lod: TextureLod;
  bytes: number;
  used: number;
}

/**
 * Pin textures (T-103): one texture per image key at the level of detail the camera asks for, inside a byte budget.
 * Signed media URLs are never used as keys or logged; a failed load is retried on the next request.
 */
export function createTextureCache<T extends Texture>({
  load,
  budgetBytes = BOARD_TEXTURE_BUDGET_BYTES,
}: TextureCacheDeps<T>): TextureCache<T> {
  const slots = new Map<string, Slot<T>>();
  const inFlight = new Map<string, Promise<void>>();
  const listeners = new Set<() => void>();
  let clock = 0;
  const notify = () => listeners.forEach((listener) => listener());
  const total = () => [...slots.values()].reduce((sum, slot) => sum + slot.bytes, 0);

  function evict(protectedKeys: ReadonlySet<string>): boolean {
    let evicted = false;
    let bytes = total();
    const oldest = [...slots.entries()]
      .filter(([key]) => !protectedKeys.has(key))
      .sort((a, b) => a[1].used - b[1].used);
    for (const [key, slot] of oldest) {
      if (bytes <= budgetBytes) break;
      slots.delete(key);
      bytes -= slot.bytes;
      evicted = true;
    }
    return evicted;
  }

  function loadOne(request: TextureRequest): Promise<void> {
    const flightKey = `${request.key}@${request.lod}`;
    const pending = inFlight.get(flightKey);
    if (pending) return pending;
    const promise = load(request.url, request.lod)
      .catch(() => null)
      .then((texture) => {
        inFlight.delete(flightKey);
        if (!texture) return;
        const current = slots.get(request.key);
        if (current && current.lod > request.lod) return;
        slots.set(request.key, {
          texture,
          lod: request.lod,
          bytes: textureBytes(texture.width(), texture.height()),
          used: ++clock,
        });
      });
    inFlight.set(flightKey, promise);
    return promise;
  }

  return {
    get: (key) => slots.get(key)?.texture,
    async ensure(requests) {
      const keys = new Set(requests.map((r) => r.key));
      const loads: Promise<void>[] = [];
      for (const request of requests) {
        const slot = slots.get(request.key);
        if (slot) slot.used = ++clock;
        if (!slot || slot.lod < request.lod) loads.push(loadOne(request));
      }
      if (loads.length === 0) return;
      await Promise.all(loads);
      evict(keys);
      notify();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    bytesInUse: total,
  };
}
