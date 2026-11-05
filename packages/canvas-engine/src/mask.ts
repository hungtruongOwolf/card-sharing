/**
 * 8-bit subject mask, the source of truth for every cut style (subject-cutout.md): row-major, one byte per pixel,
 * 0 = background, 255 = subject. Segmenters return masks smaller than the photo; geometry stays in mask pixels
 * and the renderer scales it.
 */
export interface Mask {
  readonly width: number;
  readonly height: number;
  readonly data: Uint8Array;
}

export function createMask(width: number, height: number, data?: Uint8Array): Mask {
  if (!Number.isSafeInteger(width) || !Number.isSafeInteger(height) || width < 1 || height < 1) {
    throw new RangeError(`invalid mask size ${width}x${height}`);
  }
  const pixels = data ?? new Uint8Array(width * height);
  if (pixels.length !== width * height) {
    throw new RangeError(`mask data has ${pixels.length} bytes, expected ${width * height}`);
  }
  return { width, height, data: pixels };
}

/** Binary min filter with a 3x3 square, repeated `radius` times (separable: rows then columns). */
export function erode(mask: Mask, radius = 1): Mask {
  let current = mask.data;
  for (let pass = 0; pass < radius; pass++) {
    current = minFilterColumns(
      minFilterRows(current, mask.width, mask.height),
      mask.width,
      mask.height,
    );
  }
  return { ...mask, data: current === mask.data ? mask.data.slice() : current };
}

function minFilterRows(src: Uint8Array, width: number, height: number): Uint8Array {
  const out = new Uint8Array(src.length);
  for (let y = 0; y < height; y++) {
    const row = y * width;
    for (let x = 0; x < width; x++) {
      // Outside the image counts as background, so the subject also erodes away from the border.
      const left = x > 0 ? src[row + x - 1]! : 0;
      const right = x < width - 1 ? src[row + x + 1]! : 0;
      out[row + x] = Math.min(left, src[row + x]!, right);
    }
  }
  return out;
}

function minFilterColumns(src: Uint8Array, width: number, height: number): Uint8Array {
  const out = new Uint8Array(src.length);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const i = y * width + x;
      const up = y > 0 ? src[i - width]! : 0;
      const down = y < height - 1 ? src[i + width]! : 0;
      out[i] = Math.min(up, src[i]!, down);
    }
  }
  return out;
}

/** Separable Gaussian blur with sigma = `radius` px (0 returns a copy); edges clamp. */
export function feather(mask: Mask, radius: number): Mask {
  if (!(radius > 0)) return { ...mask, data: mask.data.slice() };
  const kernel = gaussianKernel(radius);
  const { width, height } = mask;
  const horizontal = new Float32Array(width * height);
  const reach = (kernel.length - 1) / 2;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let sum = 0;
      for (let k = -reach; k <= reach; k++) {
        const sx = Math.min(width - 1, Math.max(0, x + k));
        sum += kernel[k + reach]! * mask.data[y * width + sx]!;
      }
      horizontal[y * width + x] = sum;
    }
  }
  const out = new Uint8Array(width * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let sum = 0;
      for (let k = -reach; k <= reach; k++) {
        const sy = Math.min(height - 1, Math.max(0, y + k));
        sum += kernel[k + reach]! * horizontal[sy * width + x]!;
      }
      out[y * width + x] = Math.round(sum);
    }
  }
  return { ...mask, data: out };
}

function gaussianKernel(sigma: number): Float32Array {
  const reach = Math.max(1, Math.ceil(sigma * 2));
  const kernel = new Float32Array(reach * 2 + 1);
  let total = 0;
  for (let i = -reach; i <= reach; i++) {
    const weight = Math.exp(-(i * i) / (2 * sigma * sigma));
    kernel[i + reach] = weight;
    total += weight;
  }
  return kernel.map((weight) => weight / total);
}

export interface EdgeCleanupOptions {
  /** Pixels to erode (removes the background halo segmenters leave). Default 1. */
  erodePx?: number;
  /** Feather sigma in px, 0.5-1 per effects-catalog `pre.edgeCleanup`. Default 0.75. */
  featherPx?: number;
}

/** `pre.edgeCleanup`: erode 1 px, then feather 0.5-1 px (effects-catalog.md §4). */
export function edgeCleanup(mask: Mask, options: EdgeCleanupOptions = {}): Mask {
  const { erodePx = 1, featherPx = 0.75 } = options;
  return feather(erode(mask, erodePx), featherPx);
}
