/**
 * Pin manipulation rules (T-104, design-system.md §5.3): hit testing, scale limits and rotation detents. Pure and
 * worklet-safe, so gestures run them on the UI thread.
 */
import type { PinGeometry, Vec } from './board';

/** Pinch range of a pin (design-system.md §5.3: 0.3x-4x; the server accepts 0.1-10). */
export const PIN_SCALE_LIMITS = { min: 0.3, max: 4 } as const;
/** Rotation snaps to these angles within {@link ROTATION_DETENT_DEG}. */
export const ROTATION_DETENTS = [-180, -90, 0, 90, 180] as const;
export const ROTATION_DETENT_DEG = 4;

export function clampPinScale(scale: number): number {
  'worklet';
  if (!Number.isFinite(scale)) return 1;
  return Math.min(PIN_SCALE_LIMITS.max, Math.max(PIN_SCALE_LIMITS.min, scale));
}

/** Degrees in (-180, 180]. */
export function normalizeRotation(deg: number): number {
  'worklet';
  if (!Number.isFinite(deg)) return 0;
  let r = deg % 360;
  if (r <= -180) r += 360;
  if (r > 180) r -= 360;
  return r;
}

/** The rotation after magnetic detents: within 4 degrees of 0/90/180/-90 it snaps there. */
export function snapRotation(deg: number): { rotation: number; snapped: boolean } {
  'worklet';
  const r = normalizeRotation(deg);
  for (const detent of ROTATION_DETENTS) {
    if (Math.abs(r - detent) <= ROTATION_DETENT_DEG) {
      return { rotation: normalizeRotation(detent), snapped: true };
    }
  }
  return { rotation: r, snapped: false };
}

/** True when a board point lies inside the pin's scaled, rotated frame. */
export function pinContains(pin: PinGeometry, point: Vec): boolean {
  'worklet';
  const rad = (-pin.rotation * Math.PI) / 180;
  const dx = point.x - pin.x;
  const dy = point.y - pin.y;
  const lx = dx * Math.cos(rad) - dy * Math.sin(rad);
  const ly = dx * Math.sin(rad) + dy * Math.cos(rad);
  return (
    Math.abs(lx) <= (pin.width * pin.scale) / 2 && Math.abs(ly) <= (pin.height * pin.scale) / 2
  );
}

/** Index of the topmost pin under `point` in bottom-to-top `pins`, or -1. */
export function hitTest(pins: readonly PinGeometry[], point: Vec): number {
  'worklet';
  for (let i = pins.length - 1; i >= 0; i--) {
    if (pinContains(pins[i]!, point)) return i;
  }
  return -1;
}
