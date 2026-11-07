import { createMask, type Mask } from '@card-sharing/canvas-engine';

import type { CapturedPhoto } from '../../capture';
import { maskSizeFor, type MaskSource } from '../maskSource';
import { isWorkerReply, type CutoutWorker, type WorkerReply } from './cutoutProtocol';

/**
 * Web subject masks from BiRefNet_lite running in a Web Worker (subject-cutout.md, ADR-0009). The worker, the
 * library and the model are created on the first `maskFor` call only. Every failure resolves `null`, which the
 * editor turns into a whole-photo card, so a slow or missing model never blocks making a card.
 */

/** Long side of returned masks: about the segmenter's resolution, where contouring is cheap (subject-cutout.md). */
export const CUTOUT_MASK_LONG_SIDE = 512;
/** A matte with less subject than this (fraction of pixels at or above 128) counts as "no subject" (AC-7.4). */
export const MIN_SUBJECT_FRACTION = 0.005;
/**
 * How long one photo waits for a mask. The first cut also downloads the model (~115 MB fp16, then cached by the
 * browser), so on a slow connection the first photo becomes a whole-photo card while the download continues and
 * later photos get the cutout.
 */
export const CUTOUT_TIMEOUT_MS = 45_000;
/** Recent results kept per photo URI, so the editor, export and send reuse one segmentation. */
const CACHE_SIZE = 4;

export interface BirefnetMaskSourceDeps {
  createWorker: () => CutoutWorker;
  /** Reads the photo's bytes (a same-origin `blob:` URL on the web). */
  readPhoto?: (uri: string) => Promise<Blob>;
  timeoutMs?: number;
}

export interface DisposableMaskSource extends MaskSource {
  /** Stops the worker and resolves pending requests with `null`. */
  dispose(): void;
}

const fetchPhoto = async (uri: string): Promise<Blob> => {
  const response = await fetch(uri);
  if (!response.ok) throw new Error(`photo unavailable (${response.status})`);
  return response.blob();
};

/** True when at least `MIN_SUBJECT_FRACTION` of the mask is subject. */
export function hasSubject(mask: Mask): boolean {
  let subject = 0;
  for (const value of mask.data) if (value >= 128) subject++;
  return subject >= mask.data.length * MIN_SUBJECT_FRACTION;
}

export function createBirefnetMaskSource({
  createWorker,
  readPhoto = fetchPhoto,
  timeoutMs = CUTOUT_TIMEOUT_MS,
}: BirefnetMaskSourceDeps): DisposableMaskSource {
  let worker: CutoutWorker | null = null;
  let nextId = 1;
  const pending = new Map<number, (reply: WorkerReply | null) => void>();
  const cache = new Map<string, Promise<Mask | null>>();

  const settleAll = () => {
    for (const settle of pending.values()) settle(null);
    pending.clear();
  };

  const stop = () => {
    worker?.terminate();
    worker = null;
    settleAll();
  };

  const ensureWorker = (): CutoutWorker => {
    if (worker) return worker;
    const created = createWorker();
    created.addEventListener('message', (event) => {
      if (!isWorkerReply(event.data)) return;
      const settle = pending.get(event.data.id);
      pending.delete(event.data.id);
      settle?.(event.data);
    });
    // A worker that failed to start or crashed: drop it; the next photo starts a fresh one.
    created.addEventListener('error', () => {
      if (worker === created) stop();
    });
    worker = created;
    return created;
  };

  const segment = async (photo: CapturedPhoto): Promise<Mask | null> => {
    const { width, height } = maskSizeFor(photo, CUTOUT_MASK_LONG_SIDE);
    const image = await readPhoto(photo.uri);
    const id = nextId++;
    const reply = await new Promise<WorkerReply | null>((resolve) => {
      const timer = setTimeout(() => {
        pending.delete(id);
        resolve(null);
      }, timeoutMs);
      pending.set(id, (value) => {
        clearTimeout(timer);
        resolve(value);
      });
      ensureWorker().postMessage({ type: 'segment', id, image, width, height });
    });
    if (reply === null) throw new Error('cutout timed out or the worker stopped');
    if (reply.type === 'error') throw new Error(reply.message);
    if (reply.width !== width || reply.height !== height) return null;
    if (reply.data.length !== width * height) return null;
    const mask = createMask(width, height, reply.data);
    return hasSubject(mask) ? mask : null;
  };

  return {
    maskFor(photo) {
      const cached = cache.get(photo.uri);
      if (cached) return cached;
      const result = segment(photo).catch(() => {
        // Not a verdict on the photo (timeout, crash, unreadable): forget it so a later call can retry.
        cache.delete(photo.uri);
        return null;
      });
      cache.set(photo.uri, result);
      while (cache.size > CACHE_SIZE) cache.delete(cache.keys().next().value as string);
      return result;
    },
    dispose: stop,
  };
}
