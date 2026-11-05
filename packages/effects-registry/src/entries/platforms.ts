import type { EffectEntry } from '../schema';

/** App minimum OS: Expo SDK 57 (iOS 16.4, Android API 24). Entries only raise it when they need to. */
export const APP_MIN_IOS = '16.4';
export const APP_MIN_ANDROID = 24;

/** Runs everywhere the app runs, with no device capability required. */
export const ALL_DEVICES: EffectEntry['platforms'] = {
  ios: { min: APP_MIN_IOS, requires: [] },
  android: { min: APP_MIN_ANDROID, requires: [] },
  webPreview: 'baked',
};

/**
 * Needs an on-device subject mask (Vision on iOS 17+, ML Kit with Play services on Android);
 * without one the app offers the server cutout (subject-cutout.md).
 */
export const NEEDS_SUBJECT_MASK: EffectEntry['platforms'] = {
  ios: { min: APP_MIN_IOS, requires: ['subjectMask'], fallback: 'serverCutout' },
  android: { min: APP_MIN_ANDROID, requires: ['subjectMask'], fallback: 'serverCutout' },
  webPreview: 'baked',
};
