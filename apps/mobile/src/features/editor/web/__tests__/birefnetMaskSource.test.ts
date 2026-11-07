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
  expect(readPhoto).toHaveBeenCalledTimes(1);
});

it('refuses a reply of the wrong size', async () => {
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
});

it('treats a worker error as a whole-photo card and retries the photo later', async () => {
  const fake = fakeWorker();
  const readPhoto = jest.fn(async () => blob);
  const source = createBirefnetMaskSource({ createWorker: () => fake.worker, readPhoto });
  const first = source.maskFor(photo());
  await flush();
  fake.emit('message', { type: 'error', id: fake.requests[0]?.id, message: 'download failed' });
  await expect(first).resolves.toBeNull();

  void source.maskFor(photo());
  await flush();
  expect(readPhoto).toHaveBeenCalledTimes(2);
});

it('gives up after the timeout without stopping the worker (the model keeps loading)', async () => {
  jest.useFakeTimers();
  try {
    const fake = fakeWorker();
    const source = createBirefnetMaskSource({
      createWorker: () => fake.worker,
      readPhoto: async () => blob,
      timeoutMs: 1000,
    });
    const result = source.maskFor(photo());
    await jest.advanceTimersByTimeAsync(1000);
    await expect(result).resolves.toBeNull();
    expect(fake.requests).toHaveLength(1);
    expect(fake.state.terminated).toBe(false);
  } finally {
    jest.useRealTimers();
  }
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
  expect(createWorker).not.toHaveBeenCalled();
});

it('hasSubject needs at least half a percent of subject pixels', () => {
  const data = new Uint8Array(1000);
  expect(hasSubject({ width: 100, height: 10, data })).toBe(false);
  data.fill(200, 0, 5);
  expect(hasSubject({ width: 100, height: 10, data })).toBe(true);
});
