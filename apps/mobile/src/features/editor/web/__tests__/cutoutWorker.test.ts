import {
  CUTOUT_MODEL_ID,
  CUTOUT_WORKER_SOURCE,
  TRANSFORMERS_INTEGRITY,
  TRANSFORMERS_URL,
} from '../cutoutWorker';

it('pins transformers.js to an exact version and its SHA-384', () => {
  expect(TRANSFORMERS_URL).toMatch(
    /^https:\/\/cdn\.jsdelivr\.net\/npm\/@huggingface\/transformers@\d+\.\d+\.\d+\/dist\/transformers\.min\.js$/,
  );
  expect(TRANSFORMERS_INTEGRITY).toMatch(/^sha384-[A-Za-z0-9+/]{64}$/);
  expect(CUTOUT_WORKER_SOURCE).toContain(JSON.stringify(TRANSFORMERS_INTEGRITY));
});

it('uses the MIT BiRefNet_lite model only', () => {
  expect(CUTOUT_MODEL_ID).toBe('onnx-community/BiRefNet_lite-ONNX');
  // Licensing list in subject-cutout.md: never Bria RMBG (CC BY-NC) or IMG.LY / ISNet (AGPL).
  expect(CUTOUT_WORKER_SOURCE).not.toMatch(/rmbg|briaai|imgly|isnet/i);
});

it('is syntactically valid JavaScript', () => {
  // Parses without running: the function is never called.
  expect(() => new Function(CUTOUT_WORKER_SOURCE)).not.toThrow();
});

it('runs fp32 on WASM (fp16 runs through casts there and ran out of memory, B2)', () => {
  expect(CUTOUT_WORKER_SOURCE).toContain("return { device: 'wasm', dtypes: ['fp32'] };");
  expect(CUTOUT_WORKER_SOURCE).not.toMatch(/device: 'wasm', dtypes: \[[^\]]*fp16/);
});

it('posts download progress and the start of each inference', () => {
  expect(CUTOUT_WORKER_SOURCE).toContain('progress_callback: onDownload');
  expect(CUTOUT_WORKER_SOURCE).toContain(
    "self.postMessage({ type: 'progress', stage: 'download', fraction: lastFraction });",
  );
  expect(CUTOUT_WORKER_SOURCE).toContain(
    "self.postMessage({ type: 'progress', id, stage: 'cutting' });",
  );
});
