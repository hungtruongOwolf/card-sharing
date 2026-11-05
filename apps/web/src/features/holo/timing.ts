/**
 * User Timing names for the holo spike (T-126). Kept apart from renderer.ts so the eagerly
 * loaded enlarge button can set the tap mark without importing CanvasKit.
 */
export const MARK_TAP = 'holo:tap';
export const MARK_READY = 'holo:canvaskit-ready';
export const MEASURE_LOAD = 'holo:tap-to-canvaskit-ready';
export const MEASURE_FIRST_FRAME = 'holo:tap-to-first-frame';
