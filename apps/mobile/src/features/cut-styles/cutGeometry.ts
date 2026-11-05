/**
 * Pure sizing rules for cut styles (effects-catalog.md §4). Params arrive already clamped (clampParams).
 */

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Die-cut border: `borderWidthPct` of the subject's bounding-box diagonal. */
export function borderWidthPx(subjectBounds: Rect, borderWidthPct: number): number {
  return (borderWidthPct / 100) * Math.hypot(subjectBounds.width, subjectBounds.height);
}

/** `cut.none` corner radius: `cornerRadiusPct` of the photo's shorter side. */
export function cornerRadiusPx(width: number, height: number, cornerRadiusPct: number): number {
  return (cornerRadiusPct / 100) * Math.min(width, height);
}

/**
 * Feather of the subject edge when the subject is drawn over a border (matches `pre.edgeCleanup`, 0.75 px), so
 * the photo never shows a hard aliased edge against the border.
 */
export const SUBJECT_FEATHER_PX = 0.75;
