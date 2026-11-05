import { screen } from '@testing-library/react-native';

import { themes } from '@card-sharing/tokens';

import { renderThemed, SCHEMES } from '../../test/render';
import { Text } from '../Text';

describe.each(SCHEMES)('Text (%s)', (scheme) => {
  it('applies the type token and tone colour', async () => {
    await renderThemed(
      <>
        <Text>Body</Text>
        <Text tone="muted" variant="label.sm">
          Meta
        </Text>
      </>,
      { scheme },
    );
    expect(screen.getByText('Body')).toHaveStyle({
      fontSize: 15,
      lineHeight: 22,
      color: themes[scheme].fg.ink,
    });
    expect(screen.getByText('Meta')).toHaveStyle({ fontSize: 12, color: themes[scheme].fg.muted });
  });
});

describe('Text dynamic type', () => {
  it('scales UI text to 200 % and clamps display text at 1.3x', async () => {
    await renderThemed(
      <>
        <Text>Body</Text>
        <Text variant="display.lg">Hero</Text>
      </>,
    );
    expect(screen.getByText('Body').props.maxFontSizeMultiplier).toBe(2);
    expect(screen.getByText('Hero').props.maxFontSizeMultiplier).toBe(1.3);
  });
});
