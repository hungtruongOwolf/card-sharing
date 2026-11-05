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

/** Shape cuts fill this share of the card's shorter side, leaving room for a soft edge. */
export const SHAPE_FILL = 0.96;

/** The centred square a shape cut is drawn in. */
export function centredSquare(width: number, height: number): Rect {
  const side = Math.min(width, height) * SHAPE_FILL;
  return { x: (width - side) / 2, y: (height - side) / 2, width: side, height: side };
}

export function insetRect(rect: Rect, by: number): Rect {
  const inset = Math.min(by, rect.width / 2, rect.height / 2);
  return {
    x: rect.x + inset,
    y: rect.y + inset,
    width: rect.width - 2 * inset,
    height: rect.height - 2 * inset,
  };
}

/**
 * Torn paper sizing from the card's shorter side: the paper margin around the subject (or the rim around a
 * torn photo), and the discrete path effect's segment length and deviation (scaled by `roughness`).
 */
export function tornMetrics(width: number, height: number, roughness: number) {
  const unit = Math.min(width, height);
  return {
    margin: unit * 0.045,
    segment: unit * 0.03,
    deviation: unit * 0.014 * roughness,
  };
}
