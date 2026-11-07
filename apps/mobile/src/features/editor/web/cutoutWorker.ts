import type { CutoutWorker } from './cutoutProtocol';

/**
 * The cutout Web Worker (subject-cutout.md, ADR-0009). Runs BiRefNet_lite with transformers.js off the UI thread:
 * WebGPU when the browser has it (fp16 when the GPU supports it), otherwise WASM; a GPU that loads the model but
 * cannot run it (seen on an integrated GPU: an ONNX Runtime WebGPU shader limit) switches the worker to WASM.
 *
 * Why the library is not bundled: transformers.js and ONNX Runtime Web rely on `import.meta.url`, dynamic imports and
 * their own WASM/worker files, which Metro's classic-script web bundle does not support. The worker instead loads the
 * self-contained ES module build from jsDelivr, pinned to an exact version AND to its SHA-384 (fetched with
 * Subresource Integrity, then imported from a blob URL), so a changed file is refused rather than run. ONNX Runtime's
 * WASM comes from jsDelivr (transformers.js default) and the model from the Hugging Face Hub; the browser caches
 * both (Cache API) after the first cut. The photo itself never leaves the browser.
 *
 * Licences: BiRefNet_lite weights MIT (onnx-community/BiRefNet_lite-ONNX, from ZhengPeng7/BiRefNet_lite),
 * transformers.js Apache-2.0, ONNX Runtime MIT. Never swap in CC BY-NC or AGPL models (subject-cutout.md).
 */

/** transformers.js release (2026-10-06) and the SHA-384 of `dist/transformers.min.js` at that version. */
export const TRANSFORMERS_VERSION = '4.3.1';
export const TRANSFORMERS_URL = `https://cdn.jsdelivr.net/npm/@huggingface/transformers@${TRANSFORMERS_VERSION}/dist/transformers.min.js`;
export const TRANSFORMERS_INTEGRITY =
  'sha384-AFNtlNOt2vbygtlRCUX2TDgkE+EyonjbCLxAfqezYwJJDVlozUhg+yKs4cPwyjHR';
/** Hugging Face model id (MIT). Input `input_image` 1024 x 1024, output `output_image` logits. */
export const CUTOUT_MODEL_ID = 'onnx-community/BiRefNet_lite-ONNX';

/**
 * Worker source. Protocol: `cutoutProtocol.ts`. The library and model load on the first request and are reused; a
 * failed load is retried on the next request.
 */
export const CUTOUT_WORKER_SOURCE = `
const LIBRARY_URL = ${JSON.stringify(TRANSFORMERS_URL)};
const LIBRARY_INTEGRITY = ${JSON.stringify(TRANSFORMERS_INTEGRITY)};
const MODEL_ID = ${JSON.stringify(CUTOUT_MODEL_ID)};
let library = null;
let loading = null;
// Set when WebGPU loaded the model but could not run it (some GPUs lack the shader limits it needs).
let wasmOnly = false;

async function importLibrary() {
  const response = await fetch(LIBRARY_URL, { integrity: LIBRARY_INTEGRITY, mode: 'cors', credentials: 'omit' });
  if (!response.ok) throw new Error('transformers.js unavailable: ' + response.status);
  const blob = new Blob([await response.arrayBuffer()], { type: 'text/javascript' });
  const lib = await import(URL.createObjectURL(blob));
  // Only the Hub: the app's origin serves index.html for unknown paths, never model files.
  lib.env.allowLocalModels = false;
  return lib;
}

async function backend() {
  if (!wasmOnly) {
    try {
      const adapter = await navigator.gpu?.requestAdapter();
      if (adapter) {
        return { device: 'webgpu', dtypes: adapter.features.has('shader-f16') ? ['fp16', 'fp32'] : ['fp32'] };
      }
    } catch {}
  }
  return { device: 'wasm', dtypes: ['fp16', 'fp32'] };
}

async function load() {
  if (!library) library = importLibrary().catch((error) => { library = null; throw error; });
  const lib = await library;
  const { device, dtypes } = await backend();
  let lastError = null;
  for (const dtype of dtypes) {
    try {
      const model = await lib.AutoModel.from_pretrained(MODEL_ID, { device, dtype });
      const processor = await lib.AutoProcessor.from_pretrained(MODEL_ID);
      return { lib, model, processor, device };
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError;
}

function session() {
  if (!loading) loading = load().catch((error) => { loading = null; throw error; });
  return loading;
}

async function matteOf(image, width, height) {
  const { lib, model, processor } = await session();
  const photo = await lib.RawImage.fromBlob(image);
  const { pixel_values } = await processor(photo);
  const { output_image } = await model({ input_image: pixel_values });
  return lib.RawImage.fromTensor(output_image[0].sigmoid().mul(255).to('uint8')).resize(width, height);
}

async function segment(image, width, height) {
  try {
    return await matteOf(image, width, height);
  } catch (error) {
    const current = await loading?.catch(() => null);
    if (!current || current.device !== 'webgpu') throw error;
    // WebGPU loaded but failed to run: switch to WASM for good and try once more.
    wasmOnly = true;
    loading = null;
    try { await current.model.dispose(); } catch {}
    return matteOf(image, width, height);
  }
}

self.addEventListener('message', async (event) => {
  const request = event.data;
  if (!request || request.type !== 'segment') return;
  const { id, image, width, height } = request;
  try {
    const matte = await segment(image, width, height);
    const channels = matte.channels;
    const data = new Uint8Array(width * height);
    for (let i = 0; i < data.length; i++) data[i] = matte.data[i * channels];
    self.postMessage({ type: 'mask', id, width: matte.width, height: matte.height, data }, [data.buffer]);
  } catch (error) {
    self.postMessage({ type: 'error', id, message: String((error && error.message) || error) });
  }
});
`;

/** Starts the worker as an ES module from an in-memory script (no extra file to host). */
export function createCutoutWorker(): CutoutWorker {
  const url = URL.createObjectURL(new Blob([CUTOUT_WORKER_SOURCE], { type: 'text/javascript' }));
  try {
    return new Worker(url, { type: 'module' }) as unknown as CutoutWorker;
  } finally {
    // The worker has its own reference to the script once constructed.
    URL.revokeObjectURL(url);
  }
}
