/**
 * @jest-environment <rootDir>/jest.skia-environment.js
 */
import { act, fireEvent, screen, waitFor } from '@testing-library/react-native';
import { AccessibilityInfo, Platform } from 'react-native';
import { State } from 'react-native-gesture-handler';
import { fireGestureHandler, getByGestureTestId } from 'react-native-gesture-handler/jest-utils';

import { t } from '../../../i18n';
import { renderCards } from '../../../test/cardsHarness';
import { CardViewerScreen } from '../CardViewerScreen';

jest.mock('@shopify/react-native-skia', () => ({
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  ...require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
  useImage: () => null,
}));

beforeAll(() => {
  Object.defineProperty(Platform, 'Version', { value: '17.4', configurable: true });
});

// The story viewer (sprint 9): paging, hold to pause, swipe to close, screen-reader actions, reactions.
const STORY_TIMEOUT_MS = 60_000;

async function openStory({
  segmentMs = 60_000,
  paused = false,
}: { segmentMs?: number; paused?: boolean } = {}) {
  const onClose = jest.fn();
  const onSeen = jest.fn();
  let ids: string[] = [];
  const rendered = await renderCards(
    (apis) => (
      <CardViewerScreen
        api={apis.cards}
        cardId={ids[0]!}
        sequence={ids}
        segmentMs={segmentMs}
        paused={paused}
        reactions={['react.heart']}
        onSeen={onSeen}
        onClose={onClose}
      />
    ),
    {
      prepare: (server) => {
        const mai = server.addUser('Mai');
        const ben = server.addUser('Ben');
        server.friends.add(mai.id);
        server.friends.add(ben.id);
        ids = [
          server.addIncomingCard(mai.id, { caption: 'first card' }).id,
          server.addIncomingCard(ben.id, { caption: 'second card' }).id,
        ];
      },
    },
  );
  await screen.findByText('first card');
  return { ...rendered, ids, onClose, onSeen };
}

const tapAt = (x: number) =>
  act(async () => {
    fireGestureHandler(getByGestureTestId('story-tap'), [
      { state: State.BEGAN, x },
      { state: State.ACTIVE, x },
      { state: State.END, x },
    ]);
  });

const sleep = (ms: number) => act(() => new Promise<void>((resolve) => setTimeout(resolve, ms)));

describe('CardViewerScreen as a story', () => {
  it(
    'moves on with a tap on the end side, back on the start side, and closes after the last card',
    async () => {
      const { onClose, onSeen, ids } = await openStory();
      expect(onSeen).toHaveBeenCalledWith(expect.objectContaining({ id: ids[0] }));
      await tapAt(700);
      expect(await screen.findByText('second card')).toBeTruthy();
      await tapAt(10);
      expect(await screen.findByText('first card')).toBeTruthy();
      await tapAt(700);
      await screen.findByText('second card');
      await tapAt(700);
      expect(onClose).toHaveBeenCalledTimes(1);
    },
    STORY_TIMEOUT_MS,
  );

  it(
    'plays the next card when the segment ends',
    async () => {
      await openStory({ segmentMs: 300 });
      expect(await screen.findByText('second card', {}, { timeout: 3000 })).toBeTruthy();
    },
    STORY_TIMEOUT_MS,
  );

  it(
    'holds the card while paused (a sheet over the story)',
    async () => {
      await openStory({ segmentMs: 300, paused: true });
      await sleep(900);
      expect(screen.getByText('first card')).toBeTruthy();
    },
    STORY_TIMEOUT_MS,
  );

  it(
    'closes on a swipe down and stays on a short drag',
    async () => {
      const { onClose } = await openStory();
      await act(async () => {
        fireGestureHandler(getByGestureTestId('story-dismiss'), [
          { state: State.BEGAN, translationY: 0 },
          { state: State.ACTIVE, translationY: 30 },
          { state: State.END, translationY: 30, velocityY: 10 },
        ]);
      });
      expect(onClose).not.toHaveBeenCalled();
      await act(async () => {
        fireGestureHandler(getByGestureTestId('story-dismiss'), [
          { state: State.BEGAN, translationY: 0 },
          { state: State.ACTIVE, translationY: 200 },
          { state: State.END, translationY: 200, velocityY: 100 },
        ]);
      });
      await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    },
    STORY_TIMEOUT_MS,
  );

  it(
    'never moves on by itself with a screen reader on, and offers next / previous actions',
    async () => {
      const reader = jest.spyOn(AccessibilityInfo, 'isScreenReaderEnabled').mockResolvedValue(true);
      try {
        await openStory({ segmentMs: 200 });
        await sleep(700);
        const card = screen.getByTestId('card-viewer-card');
        expect(screen.getByText('first card')).toBeTruthy();
        expect(card.props.accessibilityActions).toEqual([
          { name: 'next', label: t('card.viewer.next') },
        ]);
        await act(async () => {
          fireEvent(card, 'accessibilityAction', { nativeEvent: { actionName: 'next' } });
        });
        expect(await screen.findByText('second card')).toBeTruthy();
      } finally {
        reader.mockRestore();
      }
    },
    STORY_TIMEOUT_MS,
  );

  it(
    'reacts privately from the story',
    async () => {
      const { server, ids, user } = await openStory();
      await user.press(screen.getByRole('button', { name: t('feed.reaction.heart') }));
      await waitFor(() => expect(server.reactions.get(ids[0]!)).toBe('react.heart'));
      expect(
        screen.getByRole('button', { name: t('feed.reaction.heart') }).props.accessibilityState,
      ).toMatchObject({ selected: true });
    },
    STORY_TIMEOUT_MS,
  );
});
