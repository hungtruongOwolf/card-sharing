import { AndroidHaptics, ImpactFeedbackStyle, NotificationFeedbackType } from 'expo-haptics';

import type { HapticKind } from '../feedback';
import { createExpoHaptics, type HapticsEngine } from '../hapticsAdapter';
import {
  createExpoSound,
  FEEDBACK_AUDIO_MODE,
  SOUND_ASSETS,
  type SoundPlayer,
} from '../soundAdapter';

const KINDS: readonly HapticKind[] = [
  'soft',
  'light',
  'medium',
  'rigid',
  'selection',
  'success',
  'error',
];

function engine(): jest.Mocked<HapticsEngine> {
  return {
    impact: jest.fn().mockResolvedValue(undefined),
    selection: jest.fn().mockResolvedValue(undefined),
    notification: jest.fn().mockResolvedValue(undefined),
    android: jest.fn().mockResolvedValue(undefined),
  };
}

describe('expo-haptics adapter', () => {
  it('maps every kind to the iOS feedback generators (design-system §3.7)', () => {
    const fake = engine();
    const haptics = createExpoHaptics({ engine: fake, os: 'ios' });
    KINDS.forEach((kind) => haptics.play(kind));
    expect(fake.impact.mock.calls.map(([style]) => style)).toEqual([
      ImpactFeedbackStyle.Soft,
      ImpactFeedbackStyle.Light,
      ImpactFeedbackStyle.Medium,
      ImpactFeedbackStyle.Rigid,
    ]);
    expect(fake.selection).toHaveBeenCalledTimes(1);
    expect(fake.notification.mock.calls).toEqual([
      [NotificationFeedbackType.Success],
      [NotificationFeedbackType.Error],
    ]);
    expect(fake.android).not.toHaveBeenCalled();
  });

  it('uses the Android constants on a current device and the fallbacks on old API levels', () => {
    const current = engine();
    const modern = createExpoHaptics({ engine: current, os: 'android', apiLevel: 35 });
    KINDS.forEach((kind) => modern.play(kind));
    expect(current.android.mock.calls.map(([type]) => type)).toEqual([
      AndroidHaptics.Keyboard_Tap,
      AndroidHaptics.Gesture_Start,
      AndroidHaptics.Gesture_End,
      AndroidHaptics.Confirm,
      AndroidHaptics.Segment_Tick,
      AndroidHaptics.Confirm,
      AndroidHaptics.Reject,
    ]);

    const oldPhone = engine();
    const legacy = createExpoHaptics({ engine: oldPhone, os: 'android', apiLevel: 29 });
    KINDS.forEach((kind) => legacy.play(kind));
    expect(oldPhone.android.mock.calls.map(([type]) => type)).toEqual([
      AndroidHaptics.Keyboard_Tap,
      AndroidHaptics.Long_Press,
      AndroidHaptics.Context_Click,
      AndroidHaptics.Context_Click,
      AndroidHaptics.Clock_Tick,
      AndroidHaptics.Context_Click,
      AndroidHaptics.Long_Press,
    ]);

    // Android 11-13: the API 30 constants, but no SEGMENT_TICK (API 34) yet.
    const mid = engine();
    createExpoHaptics({ engine: mid, os: 'android', apiLevel: 31 }).play('selection');
    expect(mid.android).toHaveBeenCalledWith(AndroidHaptics.Clock_Tick);
  });

  it('swallows a refused call and does nothing on other platforms', async () => {
    const fake = engine();
    fake.impact.mockRejectedValue(new Error('no haptics engine'));
    expect(() => createExpoHaptics({ engine: fake, os: 'ios' }).play('soft')).not.toThrow();
    await Promise.resolve();
    const web = engine();
    createExpoHaptics({ engine: web, os: 'web' }).play('soft');
    expect(Object.values(web).every((call) => call.mock.calls.length === 0)).toBe(true);
  });
});

describe('expo-audio adapter', () => {
  const setup = () => {
    const players: (SoundPlayer & { asset: number })[] = [];
    const createPlayer = jest.fn((asset: number) => {
      const player = {
        asset,
        volume: 1,
        play: jest.fn(),
        seekTo: jest.fn().mockResolvedValue(undefined),
        remove: jest.fn(),
      };
      players.push(player);
      return player;
    });
    const setAudioMode = jest.fn().mockResolvedValue(undefined);
    const sound = createExpoSound({
      assets: { squeak: 1, pop: 2, whoosh: 3 },
      createPlayer,
      setAudioMode,
      volume: 0.5,
    });
    return { sound, players, createPlayer, setAudioMode };
  };

  it('touches the audio session only on the first play, respecting the silent switch', () => {
    const { sound, createPlayer, setAudioMode, players } = setup();
    expect(createPlayer).not.toHaveBeenCalled();
    expect(setAudioMode).not.toHaveBeenCalled();
    sound.play('squeak');
    expect(setAudioMode).toHaveBeenCalledWith(FEEDBACK_AUDIO_MODE);
    expect(FEEDBACK_AUDIO_MODE).toMatchObject({
      playsInSilentMode: false,
      interruptionMode: 'mixWithOthers',
    });
    expect(players.map((p) => p.asset)).toEqual([1, 2, 3]);
    expect(players.every((p) => p.volume === 0.5)).toBe(true);
  });

  it('rewinds and plays the clip each time, and releases the players', () => {
    const { sound, players, createPlayer } = setup();
    sound.play('pop');
    sound.play('pop');
    const pop = players.find((p) => p.asset === 2)!;
    expect(pop.seekTo).toHaveBeenCalledWith(0);
    expect(pop.play).toHaveBeenCalledTimes(2);
    expect(createPlayer).toHaveBeenCalledTimes(3);
    sound.release();
    expect(players.every((p) => (p.remove as jest.Mock).mock.calls.length === 1)).toBe(true);
  });

  it('stays silent when a clip cannot load', () => {
    const sound = createExpoSound({
      assets: { squeak: 1, pop: 2, whoosh: 3 },
      createPlayer: () => {
        throw new Error('decoder');
      },
      setAudioMode: jest.fn().mockResolvedValue(undefined),
    });
    expect(() => sound.play('whoosh')).not.toThrow();
  });

  it('bundles a clip for every sound id', () => {
    expect(Object.keys(SOUND_ASSETS).sort()).toEqual(['pop', 'squeak', 'whoosh']);
    expect(Object.values(SOUND_ASSETS).every((asset) => asset !== undefined)).toBe(true);
  });
});
