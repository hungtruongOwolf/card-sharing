import { act, fireEvent, screen, waitFor } from '@testing-library/react-native';
import { AppState, Text } from 'react-native';
import { State } from 'react-native-gesture-handler';
import { fireGestureHandler, getByGestureTestId } from 'react-native-gesture-handler/jest-utils';

import { t } from '../../../i18n';
import { renderThemed } from '../../../test/render';
import {
  createFeedbackGate,
  DEFAULT_FEEDBACK_PREFS,
  FEEDBACK_EVENTS,
  MIN_HAPTIC_GAP_MS,
  parseFeedbackPrefs,
  type FeedbackPrefs,
  type PrefsStorage,
} from '../feedback';
import { FeedbackProvider } from '../FeedbackProvider';
import { FeedbackSettingsScreen } from '../FeedbackSettingsScreen';
import { Squish, squishDepth, SQUISH_MIN_SCALE_Y, SQUISH_SHAPE } from '../Squish';

// eslint-disable-next-line @typescript-eslint/no-require-imports -- jest.mock factories must use require()
jest.mock('expo-secure-store', () => require('../../../test/secureStoreMock').secureStoreMock);

describe('feedback gate', () => {
  it('plays the haptic and sound of an event, at most one haptic per 50 ms', () => {
    let clock = 1000;
    const gate = createFeedbackGate(() => clock);
    const prefs = DEFAULT_FEEDBACK_PREFS;
    expect(gate(FEEDBACK_EVENTS.squish, prefs, true)).toEqual({ haptic: 'soft', sound: 'squeak' });
    clock += MIN_HAPTIC_GAP_MS - 1;
    expect(gate(FEEDBACK_EVENTS.squish, prefs, true)).toEqual({ haptic: null, sound: null });
    clock += 100;
    expect(gate(FEEDBACK_EVENTS.squish, prefs, true).haptic).toBe('soft');
  });

  it('respects the switches and stays silent in the background', () => {
    const gate = createFeedbackGate(() => 0);
    expect(gate(FEEDBACK_EVENTS.squish, { sound: false, haptics: true }, true)).toEqual({
      haptic: 'soft',
      sound: null,
    });
    const gate2 = createFeedbackGate(() => 0);
    expect(gate2(FEEDBACK_EVENTS.squish, { sound: true, haptics: false }, true)).toEqual({
      haptic: null,
      sound: 'squeak',
    });
    expect(
      createFeedbackGate(() => 0)(FEEDBACK_EVENTS.squish, DEFAULT_FEEDBACK_PREFS, false),
    ).toEqual({ haptic: null, sound: null });
  });

  it('defaults sound and haptics on and reads stored prefs defensively', () => {
    expect(DEFAULT_FEEDBACK_PREFS).toEqual({ sound: true, haptics: true });
    expect(parseFeedbackPrefs(null)).toEqual(DEFAULT_FEEDBACK_PREFS);
    expect(parseFeedbackPrefs('{bad')).toEqual(DEFAULT_FEEDBACK_PREFS);
    expect(parseFeedbackPrefs('{"sound":false,"haptics":"no"}')).toEqual({
      sound: false,
      haptics: true,
    });
  });
});

describe('squish depth', () => {
  it('grows with the tap rate and never squashes below the cap', () => {
    expect(squishDepth(0)).toBe(1);
    expect(squishDepth(1)).toBeGreaterThan(1);
    const deepest = 1 + (SQUISH_SHAPE.y - 1) * squishDepth(100);
    expect(deepest).toBeCloseTo(SQUISH_MIN_SCALE_Y);
  });
});

function memoryStorage(initial: FeedbackPrefs = DEFAULT_FEEDBACK_PREFS) {
  let stored = initial;
  const storage: PrefsStorage & { stored: () => FeedbackPrefs } = {
    load: async () => stored,
    save: async (prefs) => {
      stored = prefs;
    },
    stored: () => stored,
  };
  return storage;
}

describe('FeedbackProvider', () => {
  beforeAll(() => {
    Object.defineProperty(AppState, 'currentState', { value: 'active', configurable: true });
  });

  it('squishes with a haptic tick and a squeak, and goes quiet when muted in settings', async () => {
    const haptics = { play: jest.fn() };
    const sound = { play: jest.fn() };
    let clock = 0;
    await renderThemed(
      <FeedbackProvider haptics={haptics} sound={sound} now={() => clock}>
        <Squish gestureTestId="card" testID="card-view">
          <Text>card</Text>
        </Squish>
        <FeedbackSettingsScreen />
      </FeedbackProvider>,
    );
    const tap = () =>
      act(async () => {
        fireGestureHandler(getByGestureTestId('card'), [
          { state: State.BEGAN },
          { state: State.ACTIVE },
          { state: State.END },
        ]);
      });
    await tap();
    expect(haptics.play).toHaveBeenCalledWith('soft');
    expect(sound.play).toHaveBeenCalledWith('squeak');

    await fireEvent(screen.getByLabelText(t('feedback.sound')), 'valueChange', false);
    sound.play.mockClear();
    haptics.play.mockClear();
    clock += 1000;
    await tap();
    expect(sound.play).not.toHaveBeenCalled();
    expect(haptics.play).toHaveBeenCalledWith('soft');
  });

  it('persists the sound toggle across a restart', async () => {
    const storage = memoryStorage();
    const view = await renderThemed(
      <FeedbackProvider storage={storage}>
        <FeedbackSettingsScreen />
      </FeedbackProvider>,
    );
    const sound = await screen.findByLabelText(t('feedback.sound'));
    expect(sound.props.value).toBe(true);
    await fireEvent(sound, 'valueChange', false);
    expect(storage.stored()).toEqual({ sound: false, haptics: true });
    view.unmount();

    await renderThemed(
      <FeedbackProvider storage={storage}>
        <FeedbackSettingsScreen />
      </FeedbackProvider>,
    );
    await waitFor(() => expect(screen.getByLabelText(t('feedback.sound')).props.value).toBe(false));
  });
});
