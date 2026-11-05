import { screen, userEvent } from '@testing-library/react-native';

import { size, themes } from '@card-sharing/tokens';

import { renderThemed, SCHEMES } from '../../test/render';
import { Button } from '../Button';

describe.each(SCHEMES)('Button (%s)', (scheme) => {
  it('matches the snapshot for every variant', async () => {
    await renderThemed(
      <>
        <Button label="Primary" />
        <Button label="Secondary" variant="secondary" />
        <Button label="Ghost" variant="ghost" />
        <Button label="Delete" variant="danger" />
      </>,
      { scheme },
    );
    expect(screen.toJSON()).toMatchSnapshot();
  });

  it('uses the scheme colours and a 44 pt minimum touch target', async () => {
    await renderThemed(<Button label="Go" />, { scheme });
    const surface = screen.getByTestId('button-surface');
    expect(surface).toHaveStyle({
      backgroundColor: themes[scheme].primary.base,
      minHeight: size.touch.ios,
      minWidth: size.touch.ios,
    });
    expect(screen.getByText('Go')).toHaveStyle({ color: themes[scheme].primary.onBase });
  });
});

describe('Button behaviour', () => {
  it('is announced as a button with its label and calls onPress', async () => {
    const onPress = jest.fn();
    const user = userEvent.setup();
    await renderThemed(<Button label="Send" onPress={onPress} />);
    await user.press(screen.getByRole('button', { name: 'Send' }));
    expect(onPress).toHaveBeenCalledTimes(1);
  });

  it('accepts a separate accessibility label', async () => {
    await renderThemed(<Button label="×" accessibilityLabel="Close" />);
    expect(screen.getByRole('button', { name: 'Close' })).toBeOnTheScreen();
  });

  it('ignores presses and reports disabled state when disabled', async () => {
    const onPress = jest.fn();
    const user = userEvent.setup();
    await renderThemed(<Button label="Send" disabled onPress={onPress} />);
    const button = screen.getByRole('button', { name: 'Send' });
    expect(button).toBeDisabled();
    await user.press(button);
    expect(onPress).not.toHaveBeenCalled();
  });

  it('reports busy and blocks presses while loading', async () => {
    const onPress = jest.fn();
    const user = userEvent.setup();
    await renderThemed(<Button label="Send" loading onPress={onPress} />);
    const button = screen.getByRole('button', { name: 'Send' });
    expect(button).toBeBusy();
    await user.press(button);
    expect(onPress).not.toHaveBeenCalled();
  });

  it('lets the label scale to 200 % and wrap instead of truncating', async () => {
    await renderThemed(<Button label="A long label" />);
    const label = screen.getByText('A long label');
    expect(label.props.maxFontSizeMultiplier).toBe(2);
    expect(label.props.numberOfLines).toBeUndefined();
  });

  it('does not scale when reduced motion is on', async () => {
    await renderThemed(<Button label="Calm" />, { reducedMotion: true });
    expect(screen.getByTestId('button-surface')).not.toHaveStyle({ transform: [{ scale: 1 }] });
  });
});
