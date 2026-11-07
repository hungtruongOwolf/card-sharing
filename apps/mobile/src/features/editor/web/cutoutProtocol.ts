/** Messages between the app and the cutout worker (`cutoutWorker.ts`). */

/** App → worker: segment `image` and return a mask of `width` x `height`. */
export interface SegmentRequest {
  readonly type: 'segment';
  readonly id: number;
  readonly image: Blob;
  readonly width: number;
  readonly height: number;
}

/** Worker → app: a single-channel 8-bit matte (0 = background, 255 = subject), row-major. */
export interface MaskReply {
  readonly type: 'mask';
  readonly id: number;
  readonly width: number;
  readonly height: number;
  readonly data: Uint8Array;
}

export interface ErrorReply {
  readonly type: 'error';
  readonly id: number;
  readonly message: string;
}

/**
 * Worker → app, while it works. `download` (no `id`) is about the shared model load, for every waiting request;
 * `fraction` is 0..1 when the size is known. `cutting` (with the request's `id`) means that photo's inference started.
 */
export interface ProgressReply {
  readonly type: 'progress';
  readonly id?: number;
  readonly stage: 'download' | 'cutting';
  readonly fraction?: number | null;
}

export type WorkerReply = MaskReply | ErrorReply;

/** The part of `Worker` the mask source uses, so tests can drive it without a browser. */
export interface CutoutWorker {
  postMessage(message: SegmentRequest): void;
  addEventListener(type: 'message', listener: (event: { data: unknown }) => void): void;
  addEventListener(type: 'error', listener: (event: unknown) => void): void;
  terminate(): void;
}

export function isProgressReply(value: unknown): value is ProgressReply {
  if (!value || typeof value !== 'object') return false;
  const reply = value as Record<string, unknown>;
  if (reply.type !== 'progress') return false;
  if (reply.id !== undefined && typeof reply.id !== 'number') return false;
  if (reply.stage === 'cutting') return typeof reply.id === 'number';
  const { fraction } = reply;
  return (
    reply.stage === 'download' &&
    (fraction === undefined ||
      fraction === null ||
      (typeof fraction === 'number' && fraction >= 0 && fraction <= 1))
  );
}

export function isWorkerReply(value: unknown): value is WorkerReply {
  if (!value || typeof value !== 'object') return false;
  const reply = value as Record<string, unknown>;
  if (typeof reply.id !== 'number') return false;
  if (reply.type === 'error') return typeof reply.message === 'string';
  return (
    reply.type === 'mask' &&
    Number.isSafeInteger(reply.width) &&
    Number.isSafeInteger(reply.height) &&
    reply.data instanceof Uint8Array
  );
}
