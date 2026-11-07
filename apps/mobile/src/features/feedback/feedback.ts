import * as SecureStore from '../../platform/secureStore';

/**
 * Touch feedback (T-66, design-system.md §3.7): events map to a haptic and an optional sound. Screens emit events,
 * never haptic or sound calls, so the map stays in one place and new events are one line.
 */
export type HapticKind = 'soft' | 'light' | 'medium' | 'rigid' | 'selection' | 'success' | 'error';
export type SoundId = 'squeak' | 'pop' | 'whoosh';

export interface FeedbackEvent {
  haptic: HapticKind | null;
  sound: SoundId | null;
}

/** design-system.md §3.7 (the events built so far). */
export const FEEDBACK_EVENTS = {
  squish: { haptic: 'soft', sound: 'squeak' },
  toggle: { haptic: 'selection', sound: null },
  sendSuccess: { haptic: 'success', sound: 'whoosh' },
  error: { haptic: 'error', sound: null },
} as const satisfies Record<string, FeedbackEvent>;

export type FeedbackEventName = keyof typeof FEEDBACK_EVENTS;

/** design-system.md §3.7: at most one haptic per 50 ms (coalesced during repeated taps and drags). */
export const MIN_HAPTIC_GAP_MS = 50;
/** Sounds coalesce a little more, so a burst of taps does not stack squeaks. */
export const MIN_SOUND_GAP_MS = 80;

/**
 * Haptics engine seam. The planned adapter is expo-haptics, whose native calls follow the OS haptics setting
 * (iOS System Haptics, Android touch feedback); it is not a dependency yet, so the app ships the no-op adapter.
 */
export interface HapticsPort {
  play(kind: HapticKind): void;
}

/**
 * Sound seam. The planned adapter is expo-audio with "play in silent mode" off, so the ring/silent switch mutes it.
 * Not a dependency yet: the app ships the no-op adapter.
 */
export interface SoundPort {
  play(sound: SoundId): void;
}

export const noopHaptics: HapticsPort = { play: () => undefined };
export const noopSound: SoundPort = { play: () => undefined };

export interface FeedbackPrefs {
  /** Card-touch sounds (on by default, EM 2026-11-04: ship in MVP with a toggle). */
  sound: boolean;
  /** App-level haptics switch, on top of the OS setting. */
  haptics: boolean;
}

export const DEFAULT_FEEDBACK_PREFS: FeedbackPrefs = { sound: true, haptics: true };

/** Coalescing gate: decides which emits actually play, given the clock and the user's switches. */
export function createFeedbackGate(now: () => number = () => Date.now()) {
  let lastHaptic = -Infinity;
  let lastSound = -Infinity;
  return (
    event: FeedbackEvent,
    prefs: FeedbackPrefs,
    active: boolean,
  ): { haptic: HapticKind | null; sound: SoundId | null } => {
    // No feedback while the app is in the background (design-system.md §3.7).
    if (!active) return { haptic: null, sound: null };
    const at = now();
    let haptic: HapticKind | null = null;
    let sound: SoundId | null = null;
    if (event.haptic && prefs.haptics && at - lastHaptic >= MIN_HAPTIC_GAP_MS) {
      haptic = event.haptic;
      lastHaptic = at;
    }
    if (event.sound && prefs.sound && at - lastSound >= MIN_SOUND_GAP_MS) {
      sound = event.sound;
      lastSound = at;
    }
    return { haptic, sound };
  };
}

/** Preference storage seam (SecureStore by default: the only persistent store the app ships today). */
export interface PrefsStorage {
  load(): Promise<FeedbackPrefs>;
  save(prefs: FeedbackPrefs): Promise<void>;
}

export const FEEDBACK_PREFS_KEY = 'cs.prefs.feedback.v1';

export function parseFeedbackPrefs(raw: string | null): FeedbackPrefs {
  if (!raw) return DEFAULT_FEEDBACK_PREFS;
  try {
    const value: unknown = JSON.parse(raw);
    if (typeof value !== 'object' || value === null) return DEFAULT_FEEDBACK_PREFS;
    const record = value as Record<string, unknown>;
    return {
      sound: typeof record.sound === 'boolean' ? record.sound : DEFAULT_FEEDBACK_PREFS.sound,
      haptics:
        typeof record.haptics === 'boolean' ? record.haptics : DEFAULT_FEEDBACK_PREFS.haptics,
    };
  } catch {
    return DEFAULT_FEEDBACK_PREFS;
  }
}

export function securePrefsStorage(): PrefsStorage {
  const options: SecureStore.SecureStoreOptions = {
    keychainAccessible: SecureStore.AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY,
  };
  return {
    load: async () =>
      parseFeedbackPrefs(await SecureStore.getItemAsync(FEEDBACK_PREFS_KEY, options)),
    save: (prefs) => SecureStore.setItemAsync(FEEDBACK_PREFS_KEY, JSON.stringify(prefs), options),
  };
}
