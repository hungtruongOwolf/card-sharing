import { createMask, type Mask } from '@card-sharing/canvas-engine';

import type { CapturedPhoto } from '../../capture';
import { maskSizeFor, type CutoutProgress, type MaskResult, type MaskSource } from '../maskSource';
import {
  isProgressReply,
  isWorkerReply,
  type CutoutWorker,
  type ProgressReply,
  type WorkerReply,
} from './cutoutProtocol';

/**
 * Web subject masks from BiRefNet_lite running in a Web Worker (subject-cutout.md, ADR-0009). The worker, the
 * library and the model are created on the first call only. Every failure is reported as such (`failed`, worth a
 * retry) and never blocks making a card: the editor falls back to the whole photo and says so.
 */

/** Long side of returned masks: about the segmenter's resolution, where contouring is cheap (subject-cutout.md). */
export const CUTOUT_MASK_LONG_SIDE = 512;
/** A matte with less subject than this (fraction of pixels at or above 128) counts as "no subject" (AC-7.4). */
export const MIN_SUBJECT_FRACTION = 0.005;
/**
 * Longest silence from the worker while it loads: the library import, the model download (which reports progress,
 * each report restarts this clock, so a slow but moving download never times out) and the session creation. A
 * download that stalls this long fails.
 */
export const CUTOUT_LOAD_SILENCE_MS = 90_000;
/** How long one photo's inference may take once it started (WASM on a laptop CPU takes tens of seconds). */
export const CUTOUT_INFERENCE_TIMEOUT_MS = 120_000;
/** Recent results kept per photo URI, so the editor, export and send reuse one segmentation. */
const CACHE_SIZE = 4;

export interface BirefnetMaskSourceDeps {
  createWorker: () => CutoutWorker;
  /** Reads the photo's bytes (a same-origin `blob:` URL on the web). */
  readPhoto?: (uri: string) => Promise<Blob>;
  loadSilenceMs?: number;
  inferenceTimeoutMs?: number;
}

export interface DisposableMaskSource extends MaskSource {
  cutout(
    photo: CapturedPhoto,
    onProgress?: (progress: CutoutProgress) => void,
  ): Promise<MaskResult>;
  /** Stops the worker and fails pending requests. */
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

/** A request waiting for the worker. */
interface Pending {
  settle(reply: WorkerReply | null): void;
  progress(progress: ProgressReply): void;
}

/** One photo's segmentation, shared by every caller until it settles. */
interface Entry {
  readonly result: Promise<MaskResult>;
  readonly listeners: Set<(progress: CutoutProgress) => void>;
  last: CutoutProgress | null;
  settled: boolean;
}

const FAILED: MaskResult = { status: 'failed' };

export function createBirefnetMaskSource({
  createWorker,
  readPhoto = fetchPhoto,
  loadSilenceMs = CUTOUT_LOAD_SILENCE_MS,
  inferenceTimeoutMs = CUTOUT_INFERENCE_TIMEOUT_MS,
}: BirefnetMaskSourceDeps): DisposableMaskSource {
  let worker: CutoutWorker | null = null;
  let nextId = 1;
  const pending = new Map<number, Pending>();
  const cache = new Map<string, Entry>();

  const settleAll = () => {
    const waiting = [...pending.values()];
    pending.clear();
    for (const request of waiting) request.settle(null);
  };

  /**
   * Terminates the worker. After a failure this also frees its WASM heap, which never shrinks and may be unusable
   * after `std::bad_alloc`; the next photo starts a fresh worker and the browser serves the model from its cache.
   */
  const stop = () => {
    worker?.terminate();
    worker = null;
    settleAll();
  };

  const ensureWorker = (): CutoutWorker => {
    if (worker) return worker;
    const created = createWorker();
    created.addEventListener('message', (event) => {
      if (worker !== created) return;
      const message = event.data;
      if (isProgressReply(message)) {
        if (message.id === undefined)
          for (const request of pending.values()) request.progress(message);
        else pending.get(message.id)?.progress(message);
        return;
      }
      if (!isWorkerReply(message)) return;
      const request = pending.get(message.id);
      pending.delete(message.id);
      request?.settle(message);
    });
    // A worker that failed to start or crashed: drop it; the next photo starts a fresh one.
    created.addEventListener('error', () => {
      if (worker === created) stop();
    });
    worker = created;
    return created;
  };

  const segment = async (
    photo: CapturedPhoto,
    emit: (progress: CutoutProgress) => void,
  ): Promise<MaskResult> => {
    const { width, height } = maskSizeFor(photo, CUTOUT_MASK_LONG_SIDE);
    const image = await readPhoto(photo.uri).catch(() => null);
    if (!image) return FAILED;
    const id = nextId++;
    const reply = await new Promise<WorkerReply | null>((resolve) => {
      let timer: ReturnType<typeof setTimeout> | undefined;
      let cutting = false;
      const arm = (ms: number) => {
        clearTimeout(timer);
        timer = setTimeout(() => {
          // Silent too long while loading, or the run itself is stuck: free the worker for the next try.
          if (pending.delete(id)) stop();
          resolve(null);
        }, ms);
      };
      pending.set(id, {
        settle(value) {
          clearTimeout(timer);
          resolve(value);
        },
        progress(message) {
          if (message.stage === 'cutting') {
            cutting = true;
            arm(inferenceTimeoutMs);
            emit({ stage: 'cutting' });
          } else if (!cutting) {
            arm(loadSilenceMs);
            emit({ stage: 'download', fraction: message.fraction ?? null });
          }
        },
      });
      arm(loadSilenceMs);
      ensureWorker().postMessage({ type: 'segment', id, image, width, height });
    });
    if (reply === null) return FAILED;
    if (reply.type === 'error') {
      // e.g. `std::bad_alloc` on WASM: the worker's heap is spent, start clean next time.
      stop();
      return FAILED;
    }
    if (reply.width !== width || reply.height !== height || reply.data.length !== width * height) {
      return FAILED;
    }
    const mask = createMask(width, height, reply.data);
    return hasSubject(mask) ? { status: 'mask', mask } : { status: 'noSubject' };
  };

  const cutout = (
    photo: CapturedPhoto,
    onProgress?: (progress: CutoutProgress) => void,
  ): Promise<MaskResult> => {
    let entry = cache.get(photo.uri);
    if (!entry) {
      const listeners = new Set<(progress: CutoutProgress) => void>();
      const created: Entry = {
        listeners,
        last: null,
        settled: false,
        result: segment(photo, (progress) => {
          created.last = progress;
          for (const listener of listeners) listener(progress);
        }).then(
          (result) => {
            created.settled = true;
            listeners.clear();
            // A failure is not a verdict on the photo (time-out, crash, memory): forget it so a retry runs again.
            if (result.status === 'failed' && cache.get(photo.uri) === created)
              cache.delete(photo.uri);
            return result;
          },
          () => {
            created.settled = true;
            listeners.clear();
            if (cache.get(photo.uri) === created) cache.delete(photo.uri);
            return FAILED;
          },
        ),
      };
      entry = created;
      cache.set(photo.uri, entry);
      while (cache.size > CACHE_SIZE) cache.delete(cache.keys().next().value as string);
    }
    if (onProgress && !entry.settled) {
      entry.listeners.add(onProgress);
      if (entry.last) onProgress(entry.last);
    }
    return entry.result;
  };

  return {
    cutout,
    maskFor: (photo) =>
      cutout(photo).then((result) => (result.status === 'mask' ? result.mask : null)),
    dispose: stop,
  };
}
