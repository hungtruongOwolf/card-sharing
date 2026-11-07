import type { AudioMode } from 'expo-audio';

import type { SoundId, SoundPort } from './feedback';

/** The slice of an expo-audio player the adapter uses. */
export interface SoundPlayer {
  volume: number;
  play(): void;
  seekTo(seconds: number): Promise<void>;
  remove(): void;
}

export interface ExpoSoundOptions {
  /** Bundled assets per sound (`require` module ids); tests inject their own. */
  assets?: Readonly<Record<SoundId, number>>;
  createPlayer?: (asset: number) => SoundPlayer;
  setAudioMode?: (mode: Partial<AudioMode>) => Promise<void>;
  volume?: number;
}

/**
 * Feedback sounds (tools/dev/gen_feedback_sounds.py: synthesized in-house, no third-party license). Short, quiet
 * WAV clips; Metro bundles them as assets.
 */
export const SOUND_ASSETS: Readonly<Record<SoundId, number>> = {
  squeak: require('../../../assets/sounds/squeak.wav'),
  pop: require('../../../assets/sounds/pop.wav'),
  whoosh: require('../../../assets/sounds/whoosh.wav'),
};

/**
 * expo-audio, loaded on the first play: its native module then only starts when a sound is actually played (and
 * module-level imports stay safe where the native side is absent, such as Jest).
 */
// eslint-disable-next-line @typescript-eslint/no-require-imports -- deliberate lazy load, see above
const expoAudio = (): typeof import('expo-audio') => require('expo-audio');

/** Touch sounds sit under the content: never louder than this. */
export const FEEDBACK_VOLUME = 0.6;

/**
 * UI sounds that respect the ring/silent switch (`playsInSilentMode: false`; on Android the ringer in silent or
 * vibrate mutes them) and mix with whatever else is playing (no audio focus taken).
 */
export const FEEDBACK_AUDIO_MODE: Partial<AudioMode> = {
  playsInSilentMode: false,
  interruptionMode: 'mixWithOthers',
  shouldPlayInBackground: false,
  allowsRecording: false,
};

/**
 * The real sound engine (#246) behind the {@link SoundPort} seam. The audio mode is set and the three clips are
 * loaded on the first play, so an app that never plays a sound never touches the audio session. Replaying a clip
 * rewinds it. Failures are silent: a sound is a garnish, never a reason to break a tap.
 */
export function createExpoSound({
  assets = SOUND_ASSETS,
  createPlayer = (asset) => expoAudio().createAudioPlayer(asset),
  setAudioMode = (mode) => expoAudio().setAudioModeAsync(mode),
  volume = FEEDBACK_VOLUME,
}: ExpoSoundOptions = {}): SoundPort & { release(): void } {
  let players: Map<SoundId, SoundPlayer> | null = null;
  const load = () => {
    if (players) return players;
    players = new Map();
    try {
      void setAudioMode(FEEDBACK_AUDIO_MODE).catch(() => undefined);
    } catch {
      // No audio module: every clip below fails too and stays silent.
    }
    for (const id of Object.keys(assets) as SoundId[]) {
      try {
        const player = createPlayer(assets[id]);
        player.volume = volume;
        players.set(id, player);
      } catch {
        // A clip that cannot load just stays silent.
      }
    }
    return players;
  };
  return {
    play(sound) {
      const player = load().get(sound);
      if (!player) return;
      try {
        void player.seekTo(0).catch(() => undefined);
        player.play();
      } catch {
        // Ignore: see above.
      }
    },
    release() {
      players?.forEach((player) => player.remove());
      players = null;
    },
  };
}
