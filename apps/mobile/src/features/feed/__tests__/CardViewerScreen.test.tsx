/**
 * @jest-environment <rootDir>/jest.skia-environment.js
 */
import { Skia, type SkImage } from '@shopify/react-native-skia';
import { act, fireEvent, screen, waitFor } from '@testing-library/react-native';
import { Platform } from 'react-native';

import { t } from '../../../i18n';
import { renderCards } from '../../../test/cardsHarness';
import { problem } from '../../../test/fakeAuthServer';
import type { FakeCardsServer } from '../../../test/fakeCardsServer';
import { ME } from '../../../test/fakeSocialServer';
import { SCHEMES } from '../../../test/render';
import { cardQueryKey, CardViewerScreen } from '../CardViewerScreen';

let mockSkImage: SkImage | null = null;

jest.mock('@shopify/react-native-skia', () => ({
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  ...require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
  // The mock never loads a URL; hand the viewer a decoded picture instead.
  useImage: () => mockSkImage,
}));

beforeAll(() => {
  // jest-expo leaves Platform.Version undefined; registry routing compares it with each entry's minimum OS.
  Object.defineProperty(Platform, 'Version', { value: '17.4', configurable: true });
  const surface = Skia.Surface.Make(30, 40)!;
  surface.getCanvas().clear(Skia.Color('#3366CC'));
  surface.flush();
  mockSkImage = surface.makeImageSnapshot();
});

// Renders with CanvasKit WASM: slow under a parallel full run.
const RENDER_TIMEOUT_MS = 30_000;
const HOLO = { id: 'fx.holo', version: 1, params: { intensity: 0.5 } };

async function open({
  scheme = 'light' as const,
  reducedMotion = false,
  finish = HOLO as typeof HOLO | null,
  skewMs = 0,
  extra,
  mine = false,
  share,
  onReport,
  onReply,
  card: cardOptions,
}: {
  scheme?: 'light' | 'dark';
  reducedMotion?: boolean;
  finish?: typeof HOLO | null;
  skewMs?: number;
  extra?: (server: FakeCardsServer, cardId: string) => void;
  mine?: boolean;
  share?: (png: Uint8Array) => Promise<boolean>;
  onReport?: (cardId: string) => void;
  onReply?: (card: { id: string; sender: { id: string } }) => void;
  card?: { aiGenerated?: boolean; replyTo?: string };
} = {}) {
  const onClose = jest.fn();
  let cardId = '';
  let clock = () => 0;
  const rendered = await renderCards(
    (apis) => (
      <CardViewerScreen
        api={apis.cards}
        cardId={cardId}
        now={() => clock() + skewMs}
        loadRender={async () => mockSkImage}
        share={share}
        onReport={onReport}
        onReply={onReply}
        onClose={onClose}
      />
    ),
    {
      scheme,
      reducedMotion,
      deviceNow: (server) => () => server.now() + skewMs,
      prepare: (server) => {
        clock = server.now;
        const mai = server.addUser('Mai');
        server.friends.add(mai.id);
        const style = {
          cut: { id: 'cut.none', version: 1, params: {} },
          ...(finish && { finish }),
        };
        cardId = (
          mine
            ? server.addIncomingCard(ME, {
                caption: 'zoomies',
                style,
                recipients: [mai.id],
                ...cardOptions,
              })
            : server.addIncomingCard(mai.id, { caption: 'zoomies', style, ...cardOptions })
        ).id;
        extra?.(server, cardId);
      },
    },
  );
  return { ...rendered, cardId, onClose };
}

describe('CardViewerScreen', () => {
  it(
    'exports only my own card, with the brand mark, through the share sheet (T-127)',
    async () => {
      const share = jest.fn(async () => true);
      await open({ mine: true, share });
      await screen.findByTestId('card-viewer-image');
      await act(async () => {
        fireEvent.press(screen.getByRole('button', { name: t('card.export') }));
      });
      await waitFor(() => expect(share).toHaveBeenCalledTimes(1));
      const png = (share.mock.calls[0] as unknown as [Uint8Array])[0];
      // PNG signature.
      expect(Array.from(png.slice(0, 4))).toEqual([0x89, 0x50, 0x4e, 0x47]);
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    "offers Report on a friend's card only, never on my own (T-141)",
    async () => {
      const onReport = jest.fn();
      const { cardId } = await open({ onReport });
      await screen.findByTestId('card-viewer-image');
      fireEvent.press(screen.getByRole('button', { name: t('report.open.card', { name: 'Mai' }) }));
      expect(onReport).toHaveBeenCalledWith(cardId);
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    "offers Reply on a friend's card and hands over the card and its sender (ADR-0010)",
    async () => {
      const onReply = jest.fn();
      const { cardId } = await open({ onReply });
      await screen.findByTestId('card-viewer-image');
      fireEvent.press(
        screen.getByRole('button', { name: t('card.reply.action', { name: 'Mai' }) }),
      );
      expect(onReply).toHaveBeenCalledTimes(1);
      const card = (
        onReply.mock.calls[0] as unknown as [{ id: string; sender: { id: string } }]
      )[0];
      expect(card.id).toBe(cardId);
      expect(typeof card.sender.id).toBe('string');
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'never offers Reply on my own card',
    async () => {
      await open({ mine: true, onReply: jest.fn() });
      await screen.findByTestId('card-viewer-image');
      expect(screen.queryByTestId('card-viewer-reply')).toBeNull();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'shows the AI badge and "replied to your card" from the server flags',
    async () => {
      await open({
        card: { aiGenerated: true, replyTo: 'c0000000-0000-4000-8000-0000000000ab' },
      });
      await screen.findByTestId('card-viewer-image');
      expect(screen.getByTestId('card-viewer-ai')).toBeTruthy();
      expect(screen.getByText(/^Replied to your card/)).toBeTruthy();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'never offers Report on my own card (the server would answer 404)',
    async () => {
      await open({ mine: true, onReport: jest.fn() });
      await screen.findByTestId('card-viewer-image');
      expect(screen.queryByRole('button', { name: t('report.open') })).toBeNull();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    "never offers export on a friend's card (AC-23.2)",
    async () => {
      await open({ share: jest.fn() });
      await screen.findByTestId('card-viewer-image');
      expect(screen.queryByRole('button', { name: t('card.export') })).toBeNull();
    },
    RENDER_TIMEOUT_MS,
  );

  it.each(SCHEMES)(
    'shows the card with a fresh picture, its alt text and a live holo finish (%s)',
    async (scheme) => {
      const { server, cardId } = await open({ scheme });
      const image = await screen.findByTestId('card-viewer-image');
      expect(image.props.source.uri).toContain('https://media.test/');
      const card = screen.getByTestId('card-viewer-card');
      expect(card.props.accessibilityRole).toBe('image');
      expect(card.props.accessibilityLabel).toBe(t('feed.card.a11y', { name: 'Mai' }));
      expect(screen.getByText('zoomies')).toBeTruthy();
      expect(screen.getByTestId('card-viewer-live-finish')).toBeTruthy();
      expect(server.count('GET', `/v1/cards/${cardId}`)).toBe(1);
    },
    RENDER_TIMEOUT_MS,
  );

  it.each([
    ['under reduced motion', { reducedMotion: true }],
    ['for a finish without a live renderer', { finish: { ...HOLO, id: 'fx.matte' } }],
  ])(
    'keeps the finish static %s',
    async (_, options) => {
      await open(options);
      expect(await screen.findByTestId('card-viewer-image')).toBeTruthy();
      expect(screen.queryByTestId('card-viewer-live-finish')).toBeNull();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'says the card was removed and closes when it is deleted while open',
    async () => {
      const { server, cardId, onClose, queryClient } = await open({ finish: null });
      await screen.findByTestId('card-viewer-image');
      server.cards.delete(cardId);
      // The periodic re-read (CARD_RECHECK_MS) finds the card gone.
      await act(() => queryClient.refetchQueries({ queryKey: cardQueryKey(cardId) }));
      expect(await screen.findByText(t('card.removed'))).toBeTruthy();
      expect(onClose).toHaveBeenCalledTimes(1);
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'closes with the same message for a card I can no longer see',
    async () => {
      const { onClose } = await open({
        finish: null,
        extra: (server, cardId) => server.cards.delete(cardId),
      });
      expect(await screen.findByText(t('card.removed'))).toBeTruthy();
      expect(onClose).toHaveBeenCalledTimes(1);
      expect(screen.queryByTestId('card-viewer-image')).toBeNull();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    're-reads the card when a picture fails, at most once a minute',
    async () => {
      const { server, cardId } = await open({ finish: null });
      const image = await screen.findByTestId('card-viewer-image');
      fireEvent(image, 'error');
      await waitFor(() => expect(server.count('GET', `/v1/cards/${cardId}`)).toBe(2));
      fireEvent(screen.getByTestId('card-viewer-image'), 'error');
      expect(server.count('GET', `/v1/cards/${cardId}`)).toBe(2);
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'shows an error with a retry when the card cannot be read',
    async () => {
      const { user } = await open({
        finish: null,
        extra: (server, cardId) => {
          // The first read and its one retry fail.
          const routes = server.auth.routes!;
          let failures = 2;
          server.auth.routes = (request) =>
            request.path === `/v1/cards/${cardId}` && failures-- > 0
              ? problem(503, 'TEMPORARILY_UNAVAILABLE')
              : routes(request);
        },
      });
      expect(await screen.findByTestId('card-viewer-error', {}, { timeout: 5000 })).toBeTruthy();
      await user.press(screen.getByRole('button', { name: t('feed.retry') }));
      expect(await screen.findByTestId('card-viewer-image')).toBeTruthy();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'shows the picture on a device whose clock runs an hour ahead (#263)',
    async () => {
      await open({ finish: null, skewMs: 3_600_000 });
      expect(await screen.findByTestId('card-viewer-image')).toBeTruthy();
      expect(screen.queryByTestId('card-viewer-placeholder')).toBeNull();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'closes from the close button',
    async () => {
      const { user, onClose } = await open({ finish: null });
      await screen.findByTestId('card-viewer-image');
      await user.press(screen.getByRole('button', { name: t('card.close') }));
      expect(onClose).toHaveBeenCalledTimes(1);
    },
    RENDER_TIMEOUT_MS,
  );
});
