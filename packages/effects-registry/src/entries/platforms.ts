import type { EffectEntry } from '../schema';

/** App minimum OS: Expo SDK 58 (iOS 16.4, Android API 24). Entries only raise it when they need to. */
export const APP_MIN_IOS = '16.4';
export const APP_MIN_ANDROID = 24;

/** Runs everywhere the app runs (Skia renders on the web through CanvasKit), with no device capability required. */
export const ALL_DEVICES: EffectEntry['platforms'] = {
  ios: { min: APP_MIN_IOS, requires: [] },
  android: { min: APP_MIN_ANDROID, requires: [] },
  web: { requires: [] },
  webPreview: 'baked',
};

/**
 * Needs an on-device subject mask: Vision on iOS 17+, ML Kit or the bundled fallback model on Android
 * (prd-mvp AC-7.6), BiRefNet in a Web Worker on the web (ADR-0009). There is no server cutout in the MVP. Without a
 * mask (iOS < 17, Android without the model, a failed web cutout, no subject found, or "Use whole photo") the card is
 * a whole-photo card: these entries are hidden and the picker lists the mask-free styles, `ALL_DEVICES` entries
 * such as the shape cuts (EM decision 2026-11-05).
 */
export const NEEDS_SUBJECT_MASK: EffectEntry['platforms'] = {
  ios: { min: APP_MIN_IOS, requires: ['subjectMask'] },
  android: { min: APP_MIN_ANDROID, requires: ['subjectMask'] },
  web: { requires: ['subjectMask'] },
  webPreview: 'baked',
};

/**
 * Anchored to the face (eyes, mouth, head). Without a landmark detector the user taps the points instead
 * (`userPoints`); the web has no landmark detector yet, so it always takes the tap path.
 */
export const NEEDS_FACE_LANDMARKS: EffectEntry['platforms'] = {
  ios: { min: APP_MIN_IOS, requires: ['faceLandmarks'], fallback: 'userPoints' },
  android: { min: APP_MIN_ANDROID, requires: ['faceLandmarks'], fallback: 'userPoints' },
  web: { requires: ['faceLandmarks'], fallback: 'userPoints' },
  webPreview: 'baked',
};
