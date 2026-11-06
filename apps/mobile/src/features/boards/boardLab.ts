import { Skia, type SkImage } from '@shopify/react-native-skia';

import { BOARD_SIZE, type Board, type BoardPin } from './boardModel';

/**
 * Spike #182 lab board: N synthetic baked stickers on one canvas, the first `holo` of them with the holo finish
 * (the registry still caps how many animate). Deterministic, so frame-time runs are comparable across devices.
 */
export const BOARD_LAB_KNOBS = {
  pins: [10, 25, 50, 100],
  holo: [0, 1, 3, 6],
} as const;

export interface BoardLabSettings {
  pins: number;
  holo: number;
  forceReducedMotion: boolean;
  capture: boolean;
}

export const DEFAULT_BOARD_LAB_SETTINGS: BoardLabSettings = {
  pins: 50,
  holo: 3,
  forceReducedMotion: false,
  capture: true,
};

export const LAB_URL_PREFIX = 'lab://sticker/';
const COLUMNS = 10;
const STEP = 230;

/** Base-62 z keys `a00`, `a01`, ...: ordered like the fractional index the server stores. */
const DIGITS = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';
export const labZKey = (i: number) => `a${DIGITS[Math.floor(i / 62) % 62]}${DIGITS[i % 62]}`;

/** Small deterministic jitter so the board looks hand-placed. */
const jitter = (i: number, salt: number) =>
  (((i * 9301 + salt * 49297) % 233280) / 233280 - 0.5) * 2;

/** `title` is shown by the lab screen only. */
export function labBoard(pins: number, holo: number, title = ''): Board {
  const items: BoardPin[] = [];
  for (let i = 0; i < pins; i++) {
    const id = `00000000-0000-7000-8000-${String(i).padStart(12, '0')}`;
    items.push({
      id,
      x: 200 + (i % COLUMNS) * STEP + jitter(i, 1) * 30,
      y: 220 + Math.floor(i / COLUMNS) * (STEP * 1.3) + jitter(i, 2) * 30,
      scale: 0.8 + (jitter(i, 3) + 1) * 0.15,
      rotation: jitter(i, 4) * 12,
      zKey: labZKey(i),
      version: 1,
      content: {
        kind: 'card',
        card: {
          id,
          sender: { id: 'lab', displayName: null },
          caption: null,
          altText: null,
          media: { renderUrl: `${LAB_URL_PREFIX}${i}`, maskUrl: null, expiresAt: Infinity },
          finish: i < holo ? { id: 'fx.holo', version: 1, params: { intensity: 1 } } : null,
          sentAt: 0,
          isMine: true,
          status: 'active',
          hold: null,
          myReaction: null,
        },
      },
    });
  }
  return {
    id: 'board-lab',
    title,
    backdrop: null,
    version: 1,
    pins: items,
  };
}

/** The lab board's extent, for zoom to fit. */
export const LAB_AREA = { x: 0, y: 0, width: BOARD_SIZE.width, height: BOARD_SIZE.height };

/** Bakes the synthetic sticker for `url` at `size` px: a die-cut disc with a white border, hue by index. */
export function bakeLabSticker(url: string, size: number): SkImage | null {
  const index = Number(url.slice(LAB_URL_PREFIX.length));
  if (!url.startsWith(LAB_URL_PREFIX) || !Number.isInteger(index)) return null;
  const surface = Skia.Surface.Make(size, Math.round(size * 1.25));
  if (!surface) return null;
  const canvas = surface.getCanvas();
  const paint = Skia.Paint();
  paint.setAntiAlias(true);
  const cx = size / 2;
  const cy = (size * 1.25) / 2;
  paint.setColor(Skia.Color('#FFFFFF'));
  canvas.drawCircle(cx, cy, size * 0.48, paint);
  paint.setColor(Skia.Color(`hsl(${(index * 47) % 360}, 70%, 55%)`));
  canvas.drawCircle(cx, cy, size * 0.42, paint);
  paint.setColor(Skia.Color('rgba(255,255,255,0.35)'));
  canvas.drawCircle(cx - size * 0.12, cy - size * 0.14, size * 0.12, paint);
  surface.flush();
  return surface.makeImageSnapshot();
}
