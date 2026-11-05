/**
 * Knobs and pure helpers for the holo lab (T-52 spike). The knobs are the levers the write-up
 * (context/raw/research/2026-11-05-holo-spike.md) proposes for the effect registry `cost` field.
 */

/** How a card is drawn. */
export type HoloMode =
  /** Shader evaluated every frame, driven by gyro tilt, squish and (optionally) time. */
  | 'live'
  /** Shader rendered once to an image at a fixed tilt; drawing it is one texture sample per pixel. */
  | 'baked'
  /** Reduced motion: the shader drawn once at a fixed tilt with no animated inputs (static sheen). */
  | 'static';

export const HOLO_KNOBS = {
  cardCount: [3, 6],
  maxLive: [0, 1, 2, 3],
  resolutionScale: [1, 0.75, 0.5],
  columns: [1, 2, 3],
} as const;

export interface HoloLabSettings {
  cardCount: number;
  maxLive: number;
  resolutionScale: number;
  columns: number;
  /** Ambient animation (time uniform). Off = the shader repaints only when the tilt or squish moves. */
  ambient: boolean;
  /** Force the reduced-motion variant regardless of the OS setting. */
  forceReducedMotion: boolean;
  /** RN Skia's per-canvas debug readout (`<Canvas debug>`). */
  skiaDebug: boolean;
  /** Frame-time capture + overlay. */
  capture: boolean;
}

/** The spike scene: 3 live cards, full resolution, two columns. */
export const DEFAULT_HOLO_LAB_SETTINGS: HoloLabSettings = {
  cardCount: 3,
  maxLive: 3,
  resolutionScale: 1,
  columns: 2,
  ambient: true,
  forceReducedMotion: false,
  skiaDebug: false,
  capture: true,
};

/** The fixed "flattering" tilt used for baked and reduced-motion cards. */
export const STATIC_TILT = { x: 0.35, y: -0.2 } as const;

/** Squish press-in pushes the foil this far along y (design-system.md §5.2: squish bumps tilt). */
export const SQUISH_TILT_BUMP = 0.25;

/** Press-in shape at full squish (design-system.md §5.2). */
export const SQUISH_SCALE = { x: 1.04, y: 0.94 } as const;

/**
 * Picks a mode per card: the first `maxLive` cards (in priority order, e.g. focused / most visible
 * first) are live, the rest baked. Reduced motion turns every card into a static sheen.
 */
export function planHoloModes(count: number, maxLive: number, reducedMotion: boolean): HoloMode[] {
  const live = Math.max(0, Math.min(count, Math.floor(maxLive)));
  return Array.from({ length: Math.max(0, count) }, (_, i): HoloMode => {
    if (reducedMotion) return 'static';
    return i < live ? 'live' : 'baked';
  });
}

/** Card size in dp for a grid of `columns` across `availableWidth`, at the card aspect ratio. */
export function cardSize(availableWidth: number, columns: number, gap: number, aspect: number) {
  const cols = Math.max(1, Math.floor(columns));
  const width = Math.max(0, Math.floor((availableWidth - gap * (cols - 1)) / cols));
  return { width, height: Math.round(width * aspect) };
}

/**
 * Pixels the live shader fills per frame: live cards x (width x height in px) x resolutionScale².
 * Baked and static cards cost no shader work once drawn. This is the number the GPU cost estimate
 * scales with (write-up section 3).
 */
export function liveFillPixels(
  modes: readonly HoloMode[],
  size: { width: number; height: number },
  pixelRatio: number,
  resolutionScale: number,
): number {
  const liveCards = modes.filter((m) => m === 'live').length;
  const pxPerCard =
    Math.round(size.width * pixelRatio * resolutionScale) *
    Math.round(size.height * pixelRatio * resolutionScale);
  return liveCards * pxPerCard;
}
