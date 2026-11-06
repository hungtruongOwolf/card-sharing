import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { AppState } from 'react-native';

import {
  createFeedbackGate,
  DEFAULT_FEEDBACK_PREFS,
  FEEDBACK_EVENTS,
  noopHaptics,
  noopSound,
  type FeedbackEventName,
  type FeedbackPrefs,
  type HapticsPort,
  type PrefsStorage,
  type SoundPort,
} from './feedback';

interface FeedbackContextValue {
  prefs: FeedbackPrefs;
  setPrefs: (prefs: FeedbackPrefs) => void;
  emit: (event: FeedbackEventName) => void;
}

const FeedbackContext = createContext<FeedbackContextValue | null>(null);

export interface FeedbackProviderProps {
  children: ReactNode;
  /** Where the sound/haptics switches persist; without it they last for the session. */
  storage?: PrefsStorage;
  haptics?: HapticsPort;
  sound?: SoundPort;
  now?: () => number;
}

/** Holds the feedback switches and turns events into (coalesced) haptics and sounds. */
export function FeedbackProvider({
  children,
  storage,
  haptics = noopHaptics,
  sound = noopSound,
  now,
}: FeedbackProviderProps) {
  const [prefs, setPrefsState] = useState<FeedbackPrefs>(DEFAULT_FEEDBACK_PREFS);
  const prefsRef = useRef(prefs);
  const [gate] = useState(() => createFeedbackGate(now));

  useEffect(() => {
    if (!storage) return;
    let cancelled = false;
    storage.load().then(
      (loaded) => {
        if (cancelled) return;
        prefsRef.current = loaded;
        setPrefsState(loaded);
      },
      () => undefined,
    );
    return () => {
      cancelled = true;
    };
  }, [storage]);

  const setPrefs = useCallback(
    (next: FeedbackPrefs) => {
      prefsRef.current = next;
      setPrefsState(next);
      storage?.save(next).catch(() => undefined);
    },
    [storage],
  );

  const emit = useCallback(
    (name: FeedbackEventName) => {
      const out = gate(FEEDBACK_EVENTS[name], prefsRef.current, AppState.currentState === 'active');
      if (out.haptic) haptics.play(out.haptic);
      if (out.sound) sound.play(out.sound);
    },
    [gate, haptics, sound],
  );

  const value = useMemo(() => ({ prefs, setPrefs, emit }), [prefs, setPrefs, emit]);
  return <FeedbackContext.Provider value={value}>{children}</FeedbackContext.Provider>;
}

const SILENT: FeedbackContextValue = {
  prefs: DEFAULT_FEEDBACK_PREFS,
  setPrefs: () => undefined,
  emit: () => undefined,
};

/** Feedback outside a provider (isolated component tests) is silent. */
export function useFeedback(): FeedbackContextValue {
  return useContext(FeedbackContext) ?? SILENT;
}
