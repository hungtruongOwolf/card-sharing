import {
  AndroidHaptics,
  ImpactFeedbackStyle,
  impactAsync,
  NotificationFeedbackType,
  notificationAsync,
  performAndroidHapticsAsync,
  selectionAsync,
} from 'expo-haptics';
import { Platform } from 'react-native';

import type { HapticKind, HapticsPort } from './feedback';

/** The expo-haptics calls the adapter makes (tests inject a fake). */
export interface HapticsEngine {
  impact(style: ImpactFeedbackStyle): Promise<void>;
  selection(): Promise<void>;
  notification(type: NotificationFeedbackType): Promise<void>;
  android(type: AndroidHaptics): Promise<void>;
}

export const expoHapticsEngine: HapticsEngine = {
  impact: impactAsync,
  selection: selectionAsync,
  notification: notificationAsync,
  android: performAndroidHapticsAsync,
};

type IosHaptic =
  | { kind: 'impact'; style: ImpactFeedbackStyle }
  | { kind: 'selection' }
  | { kind: 'notification'; type: NotificationFeedbackType };

/** design-system.md §3.7, iOS column (UIFeedbackGenerator; follows the System Haptics setting). */
export const IOS_HAPTICS: Record<HapticKind, IosHaptic> = {
  soft: { kind: 'impact', style: ImpactFeedbackStyle.Soft },
  light: { kind: 'impact', style: ImpactFeedbackStyle.Light },
  medium: { kind: 'impact', style: ImpactFeedbackStyle.Medium },
  rigid: { kind: 'impact', style: ImpactFeedbackStyle.Rigid },
  selection: { kind: 'selection' },
  success: { kind: 'notification', type: NotificationFeedbackType.Success },
  error: { kind: 'notification', type: NotificationFeedbackType.Error },
};

/** Android API level that added each newer `HapticFeedbackConstants` value (older ones exist on every level). */
const ANDROID_R = 30;
const ANDROID_U = 34;

/**
 * design-system.md §3.7, Android column: `View.performHapticFeedback` constants (follow the system touch-feedback
 * setting, no VIBRATE permission), with the fallback for API levels below the one that added the constant
 * (expo-haptics rejects a constant the device does not have).
 */
export const ANDROID_HAPTICS: Record<
  HapticKind,
  { type: AndroidHaptics; minApi: number; fallback: AndroidHaptics }
> = {
  soft: { type: AndroidHaptics.Keyboard_Tap, minApi: 0, fallback: AndroidHaptics.Keyboard_Tap },
  light: {
    type: AndroidHaptics.Gesture_Start,
    minApi: ANDROID_R,
    fallback: AndroidHaptics.Long_Press,
  },
  medium: {
    type: AndroidHaptics.Gesture_End,
    minApi: ANDROID_R,
    fallback: AndroidHaptics.Context_Click,
  },
  rigid: {
    type: AndroidHaptics.Confirm,
    minApi: ANDROID_R,
    fallback: AndroidHaptics.Context_Click,
  },
  selection: {
    type: AndroidHaptics.Segment_Tick,
    minApi: ANDROID_U,
    fallback: AndroidHaptics.Clock_Tick,
  },
  success: {
    type: AndroidHaptics.Confirm,
    minApi: ANDROID_R,
    fallback: AndroidHaptics.Context_Click,
  },
  error: { type: AndroidHaptics.Reject, minApi: ANDROID_R, fallback: AndroidHaptics.Long_Press },
};

export interface ExpoHapticsOptions {
  engine?: HapticsEngine;
  os?: typeof Platform.OS;
  /** Android API level (`Platform.Version` on Android). */
  apiLevel?: number;
}

/**
 * The real haptics engine (#246) behind the {@link HapticsPort} seam. Fire and forget: a device without a haptics
 * engine or a refused call is silently ignored. The OS setting is honoured by the platform calls themselves; the
 * in-app switch and coalescing stay in the feedback gate.
 */
export function createExpoHaptics({
  engine = expoHapticsEngine,
  os = Platform.OS,
  apiLevel = typeof Platform.Version === 'number' ? Platform.Version : 0,
}: ExpoHapticsOptions = {}): HapticsPort {
  const quiet = (call: Promise<void>) => void call.catch(() => undefined);
  return {
    play(kind) {
      if (os === 'android') {
        const { type, minApi, fallback } = ANDROID_HAPTICS[kind];
        quiet(engine.android(apiLevel >= minApi ? type : fallback));
        return;
      }
      if (os !== 'ios') return;
      const haptic = IOS_HAPTICS[kind];
      if (haptic.kind === 'impact') quiet(engine.impact(haptic.style));
      else if (haptic.kind === 'selection') quiet(engine.selection());
      else quiet(engine.notification(haptic.type));
    },
  };
}
