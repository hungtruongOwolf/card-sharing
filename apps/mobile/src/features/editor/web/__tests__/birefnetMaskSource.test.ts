import type { CapturedPhoto } from '../../../capture';
import { createBirefnetMaskSource, CUTOUT_MASK_LONG_SIDE, hasSubject } from '../birefnetMaskSource';
import type { CutoutWorker, SegmentRequest } from '../cutoutProtocol';

const photo = (uri = 'blob:http://localhost:8081/a'): CapturedPhoto => ({
  uri,
  width: 2000,
  height: 1000,
  origin: 'library',
});

type Listener = (event: { data: unknown }) => void;

/** A worker the test answers by hand. */
function fakeWorker() {
  const listeners: Record<string, Listener[]> = {};
  const requests: SegmentRequest[] = [];
  const state = { terminated: false };
  const worker: CutoutWorker = {
    postMessage: (message) => void requests.push(message),
    addEventListener: (type: string, listener: Listener) => {
      (listeners[type] ??= []).push(listener);
    },
    terminate: () => {
      state.terminated = true;
    },
  };
  const emit = (type: string, data: unknown) => listeners[type]?.forEach((l) => l({ data }));
  return { worker, requests, emit, state };
}

const subjectData = (width: number, height: number) => {
  const data = new Uint8Array(width * height);
  data.fill(255, 0, Math.floor(data.length / 4));
  return data;
};

const blob = {} as Blob;
const flush = () => new Promise<void>((resolve) => setImmediate(() => resolve()));

it('creates the worker lazily, on the first photo only', async () => {
  const fake = fakeWorker();
  const createWorker = jest.fn(() => fake.worker);
  const source = createBirefnetMaskSource({ createWorker, readPhoto: async () => blob });
  expect(createWorker).not.toHaveBeenCalled();

  const result = source.maskFor(photo());
  await flush();
  expect(createWorker).toHaveBeenCalledTimes(1);
  const request = fake.requests[0];
  expect(request).toMatchObject({
    type: 'segment',
    image: blob,
    width: CUTOUT_MASK_LONG_SIDE,
    height: 256,
  });

  fake.emit('message', {
    type: 'mask',
    id: request?.id,
    width: 512,
    height: 256,
    data: subjectData(512, 256),
  });
  await expect(result).resolves.toMatchObject({ width: 512, height: 256 });
});

it('returns null when no subject is found, and reuses the answer for the same photo', async () => {
  const fake = fakeWorker();
  const readPhoto = jest.fn(async () => blob);
  const source = createBirefnetMaskSource({ createWorker: () => fake.worker, readPhoto });
  const first = source.maskFor(photo());
  await flush();
  fake.emit('message', {
    type: 'mask',
    id: fake.requests[0]?.id,
    width: 512,
    height: 256,
    data: new Uint8Array(512 * 256),
  });
  await expect(first).resolves.toBeNull();
  await expect(source.maskFor(photo())).resolves.toBeNull();
  await expect(source.cutout(photo())).resolves.toEqual({ status: 'noSubject' });
  expect(readPhoto).toHaveBeenCalledTimes(1);
});

it('refuses a reply of the wrong size as a failure', async () => {
  const fake = fakeWorker();
  const source = createBirefnetMaskSource({
    createWorker: () => fake.worker,
    readPhoto: async () => blob,
  });
  const result = source.maskFor(photo());
  await flush();
  fake.emit('message', {
    type: 'mask',
    id: fake.requests[0]?.id,
    width: 100,
    height: 100,
    data: subjectData(100, 100),
  });
  await expect(result).resolves.toBeNull();
  // Not cached as a verdict: the next call asks the worker again.
  void source.cutout(photo());
  await flush();
  expect(fake.requests).toHaveLength(2);
  source.dispose();
});

it('reports a worker error as a failure, frees the worker and retries the photo later', async () => {
  const workers = [fakeWorker(), fakeWorker()];
  let created = 0;
  const readPhoto = jest.fn(async () => blob);
  const source = createBirefnetMaskSource({
    createWorker: () => workers[created++]!.worker,
    readPhoto,
  });
  const first = source.cutout(photo());
  await flush();
  workers[0]!.emit('message', {
    type: 'error',
    id: workers[0]!.requests[0]?.id,
    message: 'std::bad_alloc',
  });
  await expect(first).resolves.toEqual({ status: 'failed' });
  // The WASM heap of a worker that ran out of memory is not reused.
  expect(workers[0]!.state.terminated).toBe(true);

  void source.maskFor(photo());
  await flush();
  expect(readPhoto).toHaveBeenCalledTimes(2);
  expect(workers[1]!.requests).toHaveLength(1);
});

it('waits as long as the download moves, then times the inference separately (B2)', async () => {
  jest.useFakeTimers();
  try {
    const fake = fakeWorker();
    const source = createBirefnetMaskSource({
      createWorker: () => fake.worker,
      readPhoto: async () => blob,
      loadSilenceMs: 1000,
      inferenceTimeoutMs: 5000,
    });
    const progress: unknown[] = [];
    const result = source.cutout(photo(), (p) => progress.push(p));
    let settled = false;
    void result.then(() => (settled = true));
    await jest.advanceTimersByTimeAsync(0);
    const id = fake.requests[0]?.id;
    // A long download that keeps reporting never times out.
    for (const fraction of [0.1, 0.4, 0.8, 1]) {
      await jest.advanceTimersByTimeAsync(900);
      fake.emit('message', { type: 'progress', stage: 'download', fraction });
    }
    fake.emit('message', { type: 'progress', id, stage: 'cutting' });
    // Inference gets its own, longer budget.
    await jest.advanceTimersByTimeAsync(4000);
    expect(settled).toBe(false);
    fake.emit('message', {
      type: 'mask',
      id,
      width: 512,
      height: 256,
      data: subjectData(512, 256),
    });
    await expect(result).resolves.toMatchObject({ status: 'mask' });
    expect(progress).toEqual([
      { stage: 'download', fraction: 0.1 },
      { stage: 'download', fraction: 0.4 },
      { stage: 'download', fraction: 0.8 },
      { stage: 'download', fraction: 1 },
      { stage: 'cutting' },
    ]);
  } finally {
    jest.useRealTimers();
  }
});

it('fails a stalled download and a stuck inference, and frees the worker', async () => {
  jest.useFakeTimers();
  try {
    const workers = [fakeWorker(), fakeWorker()];
    let created = 0;
    const source = createBirefnetMaskSource({
      createWorker: () => workers[created++]!.worker,
      readPhoto: async () => blob,
      loadSilenceMs: 1000,
      inferenceTimeoutMs: 5000,
    });
    const stalled = source.cutout(photo('blob:http://localhost:8081/a'));
    await jest.advanceTimersByTimeAsync(0);
    workers[0]!.emit('message', { type: 'progress', stage: 'download', fraction: 0.2 });
    await jest.advanceTimersByTimeAsync(1000);
    await expect(stalled).resolves.toEqual({ status: 'failed' });
    expect(workers[0]!.state.terminated).toBe(true);

    const stuck = source.cutout(photo('blob:http://localhost:8081/b'));
    await jest.advanceTimersByTimeAsync(0);
    const id = workers[1]!.requests[0]?.id;
    workers[1]!.emit('message', { type: 'progress', id, stage: 'cutting' });
    await jest.advanceTimersByTimeAsync(4999);
    workers[1]!.emit('message', { type: 'progress', stage: 'download', fraction: 1 });
    await jest.advanceTimersByTimeAsync(1);
    await expect(stuck).resolves.toEqual({ status: 'failed' });
    expect(workers[1]!.state.terminated).toBe(true);
  } finally {
    jest.useRealTimers();
  }
});

it('shares one segmentation and its progress between callers of the same photo', async () => {
  const fake = fakeWorker();
  const source = createBirefnetMaskSource({
    createWorker: () => fake.worker,
    readPhoto: async () => blob,
  });
  const early: unknown[] = [];
  const late: unknown[] = [];
  const first = source.cutout(photo(), (p) => early.push(p));
  await flush();
  fake.emit('message', { type: 'progress', stage: 'download', fraction: 0.5 });
  const second = source.cutout(photo(), (p) => late.push(p));
  expect(late).toEqual([{ stage: 'download', fraction: 0.5 }]);
  fake.emit('message', {
    type: 'mask',
    id: fake.requests[0]?.id,
    width: 512,
    height: 256,
    data: new Uint8Array(512 * 256),
  });
  await expect(first).resolves.toEqual({ status: 'noSubject' });
  await expect(second).resolves.toEqual({ status: 'noSubject' });
  expect(fake.requests).toHaveLength(1);
  expect(early).toEqual([{ stage: 'download', fraction: 0.5 }]);
});

it('ignores malformed progress messages', async () => {
  const fake = fakeWorker();
  const source = createBirefnetMaskSource({
    createWorker: () => fake.worker,
    readPhoto: async () => blob,
  });
  const progress: unknown[] = [];
  void source.cutout(photo(), (p) => progress.push(p));
  await flush();
  fake.emit('message', { type: 'progress', stage: 'download', fraction: 7 });
  fake.emit('message', { type: 'progress', stage: 'cutting' });
  fake.emit('message', { type: 'progress', stage: 'unknown' });
  expect(progress).toEqual([]);
});

it('drops a crashed worker and starts a fresh one for the next photo', async () => {
  const workers = [fakeWorker(), fakeWorker()];
  let created = 0;
  const source = createBirefnetMaskSource({
    createWorker: () => workers[created++]!.worker,
    readPhoto: async () => blob,
  });
  const first = source.maskFor(photo('blob:http://localhost:8081/a'));
  await flush();
  workers[0]!.emit('error', new Error('crash'));
  await expect(first).resolves.toBeNull();
  expect(workers[0]!.state.terminated).toBe(true);

  void source.maskFor(photo('blob:http://localhost:8081/b'));
  await flush();
  expect(created).toBe(2);
  expect(workers[1]!.requests).toHaveLength(1);
});

it('returns null when the photo cannot be read, without starting the worker', async () => {
  const createWorker = jest.fn(() => fakeWorker().worker);
  const source = createBirefnetMaskSource({
    createWorker,
    readPhoto: async () => {
      throw new Error('blob revoked');
    },
  });
  await expect(source.maskFor(photo())).resolves.toBeNull();
  await expect(source.cutout(photo('blob:http://localhost:8081/c'))).resolves.toEqual({
    status: 'failed',
  });
  expect(createWorker).not.toHaveBeenCalled();
});

it('hasSubject needs at least half a percent of subject pixels', () => {
  const data = new Uint8Array(1000);
  expect(hasSubject({ width: 100, height: 10, data })).toBe(false);
  data.fill(200, 0, 5);
  expect(hasSubject({ width: 100, height: 10, data })).toBe(true);
});
