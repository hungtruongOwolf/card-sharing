/**
 * Board scene geometry (T-103, design-system.md §5.3 and §7): the camera, pin bounds, off-screen culling and
 * texture level of detail. Pure math with no Skia dependency, so the app, the share page and server-side tests
 * agree on where a pin is. Every function here is safe to call from a Reanimated worklet.
 *
 * Coordinates: pins live in board units (`x, y` = the pin's centre, `rotation` in degrees, clockwise). The camera
 * maps a board point `p` to the screen as `(p - camera) * zoom`, so `camera.x, camera.y` is the board point at the
 * top-left corner of the view.
 */

export interface Camera {
  x: number;
  y: number;
  zoom: number;
}

export interface Size {
  width: number;
  height: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Vec {
  x: number;
  y: number;
}

/** Where a pin sits: centre, uniform scale, rotation in degrees and its unscaled size in board units. */
export interface PinGeometry {
  x: number;
  y: number;
  scale: number;
  rotation: number;
  width: number;
  height: number;
}

/** Two-finger pan/zoom range of the board camera (design-system.md §5.3: 0.25x-3x). */
export const ZOOM_LIMITS = { min: 0.25, max: 3 } as const;

export function clampZoom(zoom: number): number {
  'worklet';
  if (!Number.isFinite(zoom)) return 1;
  return Math.min(ZOOM_LIMITS.max, Math.max(ZOOM_LIMITS.min, zoom));
}

export function boardToScreen(camera: Camera, point: Vec): Vec {
  'worklet';
  return { x: (point.x - camera.x) * camera.zoom, y: (point.y - camera.y) * camera.zoom };
}

export function screenToBoard(camera: Camera, point: Vec): Vec {
  'worklet';
  return { x: point.x / camera.zoom + camera.x, y: point.y / camera.zoom + camera.y };
}

/** The camera after zooming by `factor` around a screen point that stays under the fingers. */
export function zoomAround(camera: Camera, focal: Vec, factor: number): Camera {
  'worklet';
  const zoom = clampZoom(camera.zoom * factor);
  const anchor = screenToBoard(camera, focal);
  return { x: anchor.x - focal.x / zoom, y: anchor.y - focal.y / zoom, zoom };
}

/** The camera after dragging the board by a screen distance. */
export function panBy(camera: Camera, dx: number, dy: number): Camera {
  'worklet';
  return { x: camera.x - dx / camera.zoom, y: camera.y - dy / camera.zoom, zoom: camera.zoom };
}

/** The board area the view shows, grown on every side by `margin` (a fraction of the view, e.g. 0.5). */
export function visibleRect(camera: Camera, view: Size, margin = 0): Rect {
  'worklet';
  const width = view.width / camera.zoom;
  const height = view.height / camera.zoom;
  return {
    x: camera.x - width * margin,
    y: camera.y - height * margin,
    width: width * (1 + 2 * margin),
    height: height * (1 + 2 * margin),
  };
}

/** Axis-aligned bounds of a scaled, rotated pin in board units. */
export function pinBounds(pin: PinGeometry): Rect {
  'worklet';
  const rad = (pin.rotation * Math.PI) / 180;
  const cos = Math.abs(Math.cos(rad));
  const sin = Math.abs(Math.sin(rad));
  const w = pin.width * pin.scale;
  const h = pin.height * pin.scale;
  const bw = w * cos + h * sin;
  const bh = w * sin + h * cos;
  return { x: pin.x - bw / 2, y: pin.y - bh / 2, width: bw, height: bh };
}

export function intersects(a: Rect, b: Rect): boolean {
  'worklet';
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}

/** True when `inner` lies entirely inside `outer`. */
export function contains(outer: Rect, inner: Rect): boolean {
  'worklet';
  return (
    inner.x >= outer.x &&
    inner.y >= outer.y &&
    inner.x + inner.width <= outer.x + outer.width &&
    inner.y + inner.height <= outer.y + outer.height
  );
}

/** The pins whose bounds touch `area`, in their original order (draw order is the caller's). */
export function cullPins<T extends PinGeometry>(pins: readonly T[], area: Rect): T[] {
  return pins.filter((pin) => intersects(pinBounds(pin), area));
}

/** Texture sizes a pin image is decoded at (design-system.md §7: 512 px small, 1024-2048 px zoomed). */
export const TEXTURE_LODS = [512, 1024, 2048] as const;
export type TextureLod = (typeof TEXTURE_LODS)[number];

/** Below this share of the screen's long side a pin uses the small texture. */
export const SMALL_PIN_SCREEN_SHARE = 0.3;

/**
 * Texture size for a pin drawn `onScreenPx` physical pixels on its long side, on a screen `screenPx` physical
 * pixels on its long side: the small texture under 30% of the screen, otherwise the smallest level that is at
 * least as sharp as the pixels it covers.
 */
export function textureLod(onScreenPx: number, screenPx: number): TextureLod {
  if (!(onScreenPx > 0) || onScreenPx < screenPx * SMALL_PIN_SCREEN_SHARE) return 512;
  return TEXTURE_LODS.find((lod) => lod >= onScreenPx) ?? 2048;
}

/** The long side of a pin on screen, in the same units as the view. */
export function pinScreenLongSide(pin: PinGeometry, zoom: number): number {
  return Math.max(pin.width, pin.height) * pin.scale * zoom;
}

/** Bytes a decoded RGBA texture of this size takes on the GPU. */
export function textureBytes(width: number, height: number): number {
  return Math.max(0, Math.round(width)) * Math.max(0, Math.round(height)) * 4;
}

/** Union of the bounds of `pins`, or null for none. */
export function sceneBounds(pins: readonly PinGeometry[]): Rect | null {
  if (pins.length === 0) return null;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const pin of pins) {
    const b = pinBounds(pin);
    minX = Math.min(minX, b.x);
    minY = Math.min(minY, b.y);
    maxX = Math.max(maxX, b.x + b.width);
    maxY = Math.max(maxY, b.y + b.height);
  }
  return { x: minX, y: minY, width: maxX - minX, height: maxY - minY };
}

/**
 * The camera that shows `area` centred in the view with `padding` view units around it ("zoom to fit",
 * design-system.md §5.3 double-tap on empty canvas).
 */
export function fitCamera(area: Rect, view: Size, padding = 0): Camera {
  const availW = Math.max(1, view.width - padding * 2);
  const availH = Math.max(1, view.height - padding * 2);
  const zoom = clampZoom(
    Math.min(availW / Math.max(1, area.width), availH / Math.max(1, area.height)),
  );
  const cx = area.x + area.width / 2;
  const cy = area.y + area.height / 2;
  return { x: cx - view.width / 2 / zoom, y: cy - view.height / 2 / zoom, zoom };
}
