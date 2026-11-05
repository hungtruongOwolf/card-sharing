/**
 * Minimal QR Code Model 2 encoder (ISO/IEC 18004): byte mode, versions 1–10, all four error-correction
 * levels, automatic mask selection with the standard penalty rules. Written for invite links (~50 bytes,
 * version 3–4) so the app needs no QR dependency; the algorithm follows Project Nayuki's reference encoder
 * (MIT). Longer content than version 10 holds is refused rather than silently truncated.
 */

export type ErrorCorrection = 'L' | 'M' | 'Q' | 'H';

export interface QrMatrix {
  /** Modules per side (21 + 4 × (version − 1)), without the quiet zone. */
  readonly size: number;
  readonly version: number;
  /** Row-major, `true` = dark module. */
  readonly modules: readonly (readonly boolean[])[];
}

const MAX_VERSION = 10;
const ECL_ORDINAL: Record<ErrorCorrection, number> = { L: 0, M: 1, Q: 2, H: 3 };
/** Format-information bits per level (L = 01, M = 00, Q = 11, H = 10). */
const ECL_FORMAT_BITS: Record<ErrorCorrection, number> = { L: 1, M: 0, Q: 3, H: 2 };

// Indexed [level ordinal][version]; index 0 unused.
const ECC_CODEWORDS_PER_BLOCK = [
  [-1, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18],
  [-1, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26],
  [-1, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24],
  [-1, 17, 28, 22, 16, 22, 28, 26, 26, 24, 28],
] as const;
const NUM_ERROR_CORRECTION_BLOCKS = [
  [-1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4],
  [-1, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5],
  [-1, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8],
  [-1, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8],
] as const;

const PENALTY_N1 = 3;
const PENALTY_N2 = 3;
const PENALTY_N3 = 40;
const PENALTY_N4 = 10;

const bit = (value: number, index: number): boolean => ((value >>> index) & 1) !== 0;
const at = (values: readonly number[], index: number): number => {
  const value = values[index];
  if (value === undefined) throw new RangeError('qr: table index out of range');
  return value;
};

function rawDataModules(version: number): number {
  let result = (16 * version + 128) * version + 64;
  if (version >= 2) {
    const numAlign = Math.floor(version / 7) + 2;
    result -= (25 * numAlign - 10) * numAlign - 55;
    if (version >= 7) result -= 36;
  }
  return result;
}

function dataCodewords(version: number, ecl: ErrorCorrection): number {
  const o = ECL_ORDINAL[ecl];
  return (
    Math.floor(rawDataModules(version) / 8) -
    at(ECC_CODEWORDS_PER_BLOCK[o] ?? [], version) *
      at(NUM_ERROR_CORRECTION_BLOCKS[o] ?? [], version)
  );
}

// --- Reed–Solomon over GF(2^8) with the QR polynomial 0x11D ---

function gfMultiply(x: number, y: number): number {
  let z = 0;
  for (let i = 7; i >= 0; i -= 1) {
    z = (z << 1) ^ ((z >>> 7) * 0x11d);
    z ^= ((y >>> i) & 1) * x;
  }
  return z;
}

function rsDivisor(degree: number): number[] {
  const result = new Array<number>(degree).fill(0);
  result[degree - 1] = 1;
  let root = 1;
  for (let i = 0; i < degree; i += 1) {
    for (let j = 0; j < result.length; j += 1) {
      result[j] = gfMultiply(at(result, j), root);
      if (j + 1 < result.length) result[j] = at(result, j) ^ at(result, j + 1);
    }
    root = gfMultiply(root, 0x02);
  }
  return result;
}

function rsRemainder(data: readonly number[], divisor: readonly number[]): number[] {
  const result = new Array<number>(divisor.length).fill(0);
  for (const b of data) {
    const factor = b ^ (result.shift() ?? 0);
    result.push(0);
    divisor.forEach((coef, i) => {
      result[i] = at(result, i) ^ gfMultiply(coef, factor);
    });
  }
  return result;
}

function addEccAndInterleave(data: readonly number[], version: number, ecl: ErrorCorrection) {
  const o = ECL_ORDINAL[ecl];
  const numBlocks = at(NUM_ERROR_CORRECTION_BLOCKS[o] ?? [], version);
  const blockEccLen = at(ECC_CODEWORDS_PER_BLOCK[o] ?? [], version);
  const rawCodewords = Math.floor(rawDataModules(version) / 8);
  const numShortBlocks = numBlocks - (rawCodewords % numBlocks);
  const shortBlockLen = Math.floor(rawCodewords / numBlocks);

  const divisor = rsDivisor(blockEccLen);
  const blocks: number[][] = [];
  for (let i = 0, k = 0; i < numBlocks; i += 1) {
    const dat = data.slice(k, k + shortBlockLen - blockEccLen + (i < numShortBlocks ? 0 : 1));
    k += dat.length;
    const ecc = rsRemainder(dat, divisor);
    if (i < numShortBlocks) dat.push(0);
    blocks.push(dat.concat(ecc));
  }

  const result: number[] = [];
  const blockLen = blocks[0]?.length ?? 0;
  for (let i = 0; i < blockLen; i += 1) {
    blocks.forEach((block, j) => {
      // Skip the padding byte of the short blocks.
      if (i !== shortBlockLen - blockEccLen || j >= numShortBlocks) result.push(at(block, i));
    });
  }
  return result;
}

// --- Data segment (byte mode) ---

function utf8(text: string): number[] {
  const bytes: number[] = [];
  for (const char of text) {
    const cp = char.codePointAt(0) ?? 0;
    if (cp < 0x80) bytes.push(cp);
    else if (cp < 0x800) bytes.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f));
    else if (cp < 0x10000) {
      bytes.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
    } else {
      bytes.push(
        0xf0 | (cp >> 18),
        0x80 | ((cp >> 12) & 0x3f),
        0x80 | ((cp >> 6) & 0x3f),
        0x80 | (cp & 0x3f),
      );
    }
  }
  return bytes;
}

function encodeData(bytes: readonly number[], version: number, ecl: ErrorCorrection): number[] {
  const bits: number[] = [];
  const append = (value: number, length: number) => {
    for (let i = length - 1; i >= 0; i -= 1) bits.push((value >>> i) & 1);
  };
  append(0b0100, 4); // byte mode
  append(bytes.length, version <= 9 ? 8 : 16);
  for (const b of bytes) append(b, 8);

  const capacityBits = dataCodewords(version, ecl) * 8;
  append(0, Math.min(4, capacityBits - bits.length)); // terminator
  append(0, (8 - (bits.length % 8)) % 8);
  for (let pad = 0xec; bits.length < capacityBits; pad ^= 0xec ^ 0x11) append(pad, 8);

  const codewords: number[] = [];
  for (let i = 0; i < bits.length; i += 8) {
    let byte = 0;
    for (let j = 0; j < 8; j += 1) byte = (byte << 1) | at(bits, i + j);
    codewords.push(byte);
  }
  return codewords;
}

/** Smallest version whose capacity fits `byteCount` bytes, or null past {@link MAX_VERSION}. */
function chooseVersion(byteCount: number, ecl: ErrorCorrection): number | null {
  for (let version = 1; version <= MAX_VERSION; version += 1) {
    const used = 4 + (version <= 9 ? 8 : 16) + byteCount * 8;
    if (used <= dataCodewords(version, ecl) * 8) return version;
  }
  return null;
}

// --- Module placement ---

class Grid {
  readonly modules: boolean[][];
  readonly isFunction: boolean[][];
  readonly size: number;

  constructor(size: number) {
    this.size = size;
    this.modules = Array.from({ length: size }, () => new Array<boolean>(size).fill(false));
    this.isFunction = Array.from({ length: size }, () => new Array<boolean>(size).fill(false));
  }

  get(x: number, y: number): boolean {
    return this.modules[y]?.[x] ?? false;
  }

  setFunction(x: number, y: number, dark: boolean): void {
    const row = this.modules[y];
    const fn = this.isFunction[y];
    if (!row || !fn) return;
    row[x] = dark;
    fn[x] = true;
  }
}

function alignmentPositions(version: number, size: number): number[] {
  if (version === 1) return [];
  const numAlign = Math.floor(version / 7) + 2;
  const step = Math.floor((version * 8 + numAlign * 3 + 5) / (numAlign * 4 - 4)) * 2;
  const result = [6];
  for (let pos = size - 7; result.length < numAlign; pos -= step) result.splice(1, 0, pos);
  return result;
}

function drawFormatBits(grid: Grid, ecl: ErrorCorrection, mask: number): void {
  const data = (ECL_FORMAT_BITS[ecl] << 3) | mask;
  let rem = data;
  for (let i = 0; i < 10; i += 1) rem = (rem << 1) ^ ((rem >>> 9) * 0x537);
  const bits = ((data << 10) | rem) ^ 0x5412;
  const { size } = grid;

  for (let i = 0; i <= 5; i += 1) grid.setFunction(8, i, bit(bits, i));
  grid.setFunction(8, 7, bit(bits, 6));
  grid.setFunction(8, 8, bit(bits, 7));
  grid.setFunction(7, 8, bit(bits, 8));
  for (let i = 9; i < 15; i += 1) grid.setFunction(14 - i, 8, bit(bits, i));

  for (let i = 0; i < 8; i += 1) grid.setFunction(size - 1 - i, 8, bit(bits, i));
  for (let i = 8; i < 15; i += 1) grid.setFunction(8, size - 15 + i, bit(bits, i));
  grid.setFunction(8, size - 8, true); // the dark module
}

function drawVersion(grid: Grid, version: number): void {
  if (version < 7) return;
  let rem = version;
  for (let i = 0; i < 12; i += 1) rem = (rem << 1) ^ ((rem >>> 11) * 0x1f25);
  const bits = (version << 12) | rem;
  for (let i = 0; i < 18; i += 1) {
    const dark = bit(bits, i);
    const a = grid.size - 11 + (i % 3);
    const b = Math.floor(i / 3);
    grid.setFunction(a, b, dark);
    grid.setFunction(b, a, dark);
  }
}

function drawFunctionPatterns(grid: Grid, version: number, ecl: ErrorCorrection): void {
  const { size } = grid;
  for (let i = 0; i < size; i += 1) {
    grid.setFunction(6, i, i % 2 === 0);
    grid.setFunction(i, 6, i % 2 === 0);
  }
  for (const [cx, cy] of [
    [3, 3],
    [size - 4, 3],
    [3, size - 4],
  ] as const) {
    for (let dy = -4; dy <= 4; dy += 1) {
      for (let dx = -4; dx <= 4; dx += 1) {
        const dist = Math.max(Math.abs(dx), Math.abs(dy));
        const x = cx + dx;
        const y = cy + dy;
        if (x >= 0 && x < size && y >= 0 && y < size)
          grid.setFunction(x, y, dist !== 2 && dist !== 4);
      }
    }
  }
  const positions = alignmentPositions(version, size);
  const last = positions.length - 1;
  positions.forEach((px, i) => {
    positions.forEach((py, j) => {
      if ((i === 0 && j === 0) || (i === 0 && j === last) || (i === last && j === 0)) return;
      for (let dy = -2; dy <= 2; dy += 1) {
        for (let dx = -2; dx <= 2; dx += 1) {
          grid.setFunction(px + dx, py + dy, Math.max(Math.abs(dx), Math.abs(dy)) !== 1);
        }
      }
    });
  });
  drawFormatBits(grid, ecl, 0); // placeholder, overwritten once the mask is chosen
  drawVersion(grid, version);
}

function drawCodewords(grid: Grid, data: readonly number[]): void {
  const { size } = grid;
  let i = 0;
  for (let right = size - 1; right >= 1; right -= 2) {
    if (right === 6) right = 5; // skip the vertical timing column
    for (let vert = 0; vert < size; vert += 1) {
      for (let j = 0; j < 2; j += 1) {
        const x = right - j;
        const upward = ((right + 1) & 2) === 0;
        const y = upward ? size - 1 - vert : vert;
        const row = grid.modules[y];
        if (row && !grid.isFunction[y]?.[x] && i < data.length * 8) {
          row[x] = bit(at(data, i >>> 3), 7 - (i & 7));
          i += 1;
        }
      }
    }
  }
}

function maskBit(mask: number, x: number, y: number): boolean {
  switch (mask) {
    case 0:
      return (x + y) % 2 === 0;
    case 1:
      return y % 2 === 0;
    case 2:
      return x % 3 === 0;
    case 3:
      return (x + y) % 3 === 0;
    case 4:
      return (Math.floor(x / 3) + Math.floor(y / 2)) % 2 === 0;
    case 5:
      return ((x * y) % 2) + ((x * y) % 3) === 0;
    case 6:
      return (((x * y) % 2) + ((x * y) % 3)) % 2 === 0;
    default:
      return (((x + y) % 2) + ((x * y) % 3)) % 2 === 0;
  }
}

function applyMask(grid: Grid, mask: number): void {
  for (let y = 0; y < grid.size; y += 1) {
    const row = grid.modules[y];
    const fn = grid.isFunction[y];
    if (!row || !fn) continue;
    for (let x = 0; x < grid.size; x += 1) {
      if (!fn[x] && maskBit(mask, x, y)) row[x] = !row[x];
    }
  }
}

// --- Penalty (mask selection) ---

function finderPenalty(size: number) {
  const history = [0, 0, 0, 0, 0, 0, 0];
  const add = (runLength: number) => {
    const length = history[0] === 0 ? runLength + size : runLength; // light border before the row
    history.pop();
    history.unshift(length);
  };
  const count = () => {
    const [h0 = 0, h1 = 0, h2 = 0, h3 = 0, h4 = 0, h5 = 0, h6 = 0] = history;
    const core = h1 > 0 && h2 === h1 && h3 === h1 * 3 && h4 === h1 && h5 === h1;
    return (core && h0 >= h1 * 4 && h6 >= h1 ? 1 : 0) + (core && h6 >= h1 * 4 && h0 >= h1 ? 1 : 0);
  };
  return {
    reset: () => history.fill(0),
    add,
    count,
    terminate: (runColor: boolean, runLength: number) => {
      let length = runLength;
      if (runColor) {
        add(length);
        length = 0;
      }
      add(length + size); // light border after the row
      return count();
    },
  };
}

function penalty(grid: Grid): number {
  const { size } = grid;
  let result = 0;
  const finder = finderPenalty(size);
  for (const vertical of [false, true]) {
    for (let a = 0; a < size; a += 1) {
      let runColor = false;
      let run = 0;
      finder.reset();
      for (let b = 0; b < size; b += 1) {
        const color = vertical ? grid.get(a, b) : grid.get(b, a);
        if (color === runColor) {
          run += 1;
          if (run === 5) result += PENALTY_N1;
          else if (run > 5) result += 1;
        } else {
          finder.add(run);
          if (!runColor) result += finder.count() * PENALTY_N3;
          runColor = color;
          run = 1;
        }
      }
      result += finder.terminate(runColor, run) * PENALTY_N3;
    }
  }
  let dark = 0;
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      const color = grid.get(x, y);
      if (color) dark += 1;
      if (
        x < size - 1 &&
        y < size - 1 &&
        color === grid.get(x + 1, y) &&
        color === grid.get(x, y + 1) &&
        color === grid.get(x + 1, y + 1)
      ) {
        result += PENALTY_N2;
      }
    }
  }
  const total = size * size;
  const k = Math.ceil(Math.abs(dark * 20 - total * 10) / total) - 1;
  return result + k * PENALTY_N4;
}

/**
 * Encodes `text` (UTF-8, byte mode) at error-correction level `ecl`. Returns null when it does not fit in
 * version 10; the caller then shows the link without a code.
 */
export function encodeQr(text: string, ecl: ErrorCorrection = 'M'): QrMatrix | null {
  const bytes = utf8(text);
  const version = chooseVersion(bytes.length, ecl);
  if (version === null) return null;
  const codewords = addEccAndInterleave(encodeData(bytes, version, ecl), version, ecl);

  const grid = new Grid(version * 4 + 17);
  drawFunctionPatterns(grid, version, ecl);
  drawCodewords(grid, codewords);

  let best = 0;
  let bestScore = Infinity;
  for (let mask = 0; mask < 8; mask += 1) {
    applyMask(grid, mask);
    drawFormatBits(grid, ecl, mask);
    const score = penalty(grid);
    if (score < bestScore) {
      best = mask;
      bestScore = score;
    }
    applyMask(grid, mask); // XOR again to undo
  }
  applyMask(grid, best);
  drawFormatBits(grid, ecl, best);
  return { size: grid.size, version, modules: grid.modules };
}

/**
 * The dark modules as one SVG path in module units (each dark module a 1×1 square, offset by `quiet`
 * modules of quiet zone), for a single Skia `<Path>`.
 */
export function qrPath(matrix: QrMatrix, quiet = 4): string {
  const parts: string[] = [];
  matrix.modules.forEach((row, y) => {
    let x = 0;
    while (x < matrix.size) {
      if (!row[x]) {
        x += 1;
        continue;
      }
      const start = x;
      while (x < matrix.size && row[x]) x += 1;
      parts.push(`M${start + quiet} ${y + quiet}h${x - start}v1h${start - x}z`);
    }
  });
  return parts.join('');
}
