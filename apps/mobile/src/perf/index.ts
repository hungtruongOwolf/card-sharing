export { FrameTimeOverlay, type FrameTimeOverlayProps } from './FrameTimeOverlay';
export {
  createFrameHistogram,
  missedFrames,
  recordFrame,
  resetFrameHistogram,
  statsFromIntervals,
  summarizeFrames,
  type FrameHistogram,
  type FrameStats,
} from './frameStats';
export {
  useFrameTimeCapture,
  type FrameTimeCaptureOptions,
  type FrameTimeReport,
} from './useFrameTimeCapture';
