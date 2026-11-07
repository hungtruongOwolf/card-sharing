import { act, renderHook, screen, userEvent } from '@testing-library/react-native';

import { linearGradient, onMedia, size, themes } from '@card-sharing/tokens';

import { renderThemed } from '../../test/render';
import { Avatar, avatarFill, initialOf } from '../Avatar';
import { IconButton } from '../IconButton';
import { useBrandFonts } from '../theme/useBrandFonts';

describe('Avatar', () => {
  it('uses the first grapheme of the name, upper-cased, and ? for an empty name', () => {
    expect(initialOf('mai')).toBe('M');
    expect(initialOf('  linh ')).toBe('L');
    expect(initialOf('👩🏽‍🚀 Ana')).toBe('👩🏽‍🚀');
    expect(initialOf('')).toBe('?');
  });

  it('keeps one colour per friend id', () => {
    expect(avatarFill('friend-1')).toBe(avatarFill('friend-1'));
    const fills = new Set(['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'].map(avatarFill));
    expect(fills.size).toBeGreaterThan(1);
  });

  it('draws the gradient story ring when unseen and a hairline ring when seen', async () => {
    await renderThemed(
      <>
        <Avatar id="u1" name="Mai" ring="unseen" testID="unseen" />
        <Avatar id="u2" name="Linh" ring="seen" testID="seen" />
      </>,
      { scheme: 'dark' },
    );
    const hidden = { includeHiddenElements: true };
    expect(screen.getByTestId('unseen', hidden)).toHaveStyle({
      backgroundImage: linearGradient('story'),
      width: size.avatar.lg,
      padding: size.ring.stroke,
    });
    expect(screen.getByTestId('seen', hidden)).not.toHaveStyle({
      backgroundImage: linearGradient('story'),
    });
    // Decorative: the row that holds the avatar names the friend.
    expect(screen.getByText('M', hidden)).toBeOnTheScreen();
    expect(screen.queryByText('M')).toBeNull();
  });
});

describe('IconButton', () => {
  it('is a labelled button with at least the touch target, glass over photos', async () => {
    const onPress = jest.fn();
    const user = userEvent.setup();
    await renderThemed(<IconButton icon="close" accessibilityLabel="Close" onPress={onPress} />);
    await user.press(screen.getByRole('button', { name: 'Close' }));
    expect(onPress).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId('icon-button-surface')).toHaveStyle({
      width: size.touch.ios,
      backgroundColor: onMedia.glass,
    });
  });

  it('reports selected and draws the gradient variant with its solid fallback', async () => {
    await renderThemed(
      <IconButton icon="send" variant="gradient" selected accessibilityLabel="Send" size={56} />,
      { scheme: 'dark' },
    );
    expect(screen.getByRole('button', { name: 'Send' })).toBeSelected();
    expect(screen.getByTestId('icon-button-surface')).toHaveStyle({
      width: 56,
      backgroundColor: themes.dark.primary.base,
      backgroundImage: linearGradient('primary'),
    });
  });
});

describe('useBrandFonts', () => {
  beforeEach(() => jest.useFakeTimers());
  afterEach(() => jest.useRealTimers());

  it('turns ready once the fonts load', async () => {
    let resolve!: (ok: boolean) => void;
    const load = () => new Promise<boolean>((r) => (resolve = r));
    const { result } = await renderHook(() => useBrandFonts(load, 1500));
    expect(result.current).toBe(false);
    await act(async () => resolve(true));
    expect(result.current).toBe(true);
  });

  it('stops waiting after the timeout and still re-renders when the fonts arrive late', async () => {
    let resolve!: (ok: boolean) => void;
    const load = () => new Promise<boolean>((r) => (resolve = r));
    let renders = 0;
    const { result } = await renderHook(() => {
      renders++;
      return useBrandFonts(load, 1500);
    });
    await act(async () => jest.advanceTimersByTime(1500));
    expect(result.current).toBe(true);
    const before = renders;
    await act(async () => resolve(true));
    expect(renders).toBeGreaterThan(before);
  });
});
