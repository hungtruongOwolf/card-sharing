import type { EffectEntry } from '../schema';

/** App minimum OS: Expo SDK 58 (iOS 16.4, Android API 24). Entries only raise it when they need to. */
export const APP_MIN_IOS = '16.4';
export const APP_MIN_ANDROID = 24;

/** Runs everywhere the app runs, with no device capability required. */
export const ALL_DEVICES: EffectEntry['platforms'] = {
  ios: { min: APP_MIN_IOS, requires: [] },
  android: { min: APP_MIN_ANDROID, requires: [] },
  webPreview: 'baked',
};

/**
 * Needs an on-device subject mask: Vision on iOS 17+, ML Kit or the bundled fallback model on Android
 * (prd-mvp AC-7.6). There is no server cutout in the MVP. Without a mask (iOS < 17, Android without the model,
 * no subject found, or "Use whole photo") the card is a whole-photo card: these entries are hidden and the
 * picker lists the mask-free styles, `ALL_DEVICES` entries such as the shape cuts (EM decision 2026-11-05).
 */
export const NEEDS_SUBJECT_MASK: EffectEntry['platforms'] = {
  ios: { min: APP_MIN_IOS, requires: ['subjectMask'] },
  android: { min: APP_MIN_ANDROID, requires: ['subjectMask'] },
  webPreview: 'baked',
};
