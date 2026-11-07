import { deflateSync } from 'node:zlib';

/**
 * A generated test photo (PNG): a shaded orange ball with a soft shadow on a plain light backdrop. Generated rather
 * than committed so the repo holds no binary fixture. The top-left pixel is light grey, so the fake content scanners
 * (magenta/cyan/yellow/blue top-left markers, see .env.example) pass it.
 */
export function photoPng(width = 600, height = 600): Buffer {
  const stride = width * 3 + 1;
  const raw = Buffer.alloc(stride * height);
  const cx = width / 2;
  const cy = height * 0.52;
  const radius = width * 0.31;
  for (let y = 0; y < height; y++) {
    raw[y * stride] = 0; // filter: none
    for (let x = 0; x < width; x++) {
      let r = 228 - (y / height) * 40;
      let g = 226 - (y / height) * 40;
      let b = 220 - (y / height) * 40;
      const sx = (x - cx) / (radius * 0.9);
      const sy = (y - (cy + radius)) / (radius * 0.16);
      if (sx * sx + sy * sy < 1) {
        r *= 0.7;
        g *= 0.7;
        b *= 0.7;
      }
      const d = Math.hypot(x - cx, y - cy);
      if (d < radius) {
        const light = Math.max(
          0.25,
          1 - Math.hypot(x - cx + radius * 0.36, y - cy + radius * 0.36) / (radius * 1.7),
        );
        r = 250 * light + 30;
        g = 120 * light + 20;
        b = 30 * light + 10;
      }
      const offset = y * stride + 1 + x * 3;
      raw[offset] = Math.min(255, Math.round(r));
      raw[offset + 1] = Math.min(255, Math.round(g));
      raw[offset + 2] = Math.min(255, Math.round(b));
    }
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8; // bit depth
  header[9] = 2; // colour type: RGB
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(bytes: Buffer): number {
  let c = 0xffffffff;
  for (const byte of bytes) c = (CRC_TABLE[(c ^ byte) & 0xff] ?? 0) ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type: string, data: Buffer): Buffer {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}
