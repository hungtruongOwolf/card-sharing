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

export type WorkerReply = MaskReply | ErrorReply;

/** The part of `Worker` the mask source uses, so tests can drive it without a browser. */
export interface CutoutWorker {
  postMessage(message: SegmentRequest): void;
  addEventListener(type: 'message', listener: (event: { data: unknown }) => void): void;
  addEventListener(type: 'error', listener: (event: unknown) => void): void;
  terminate(): void;
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
