import { screen } from '@testing-library/react-native';

import { themes } from '@card-sharing/tokens';

import { renderThemed, SCHEMES } from '../../test/render';
import { Screen } from '../Screen';
import { Text } from '../Text';

describe.each(SCHEMES)('Screen (%s)', (scheme) => {
  it('matches the snapshot', async () => {
    await renderThemed(
      <Screen title="Friends" testID="screen">
        <Text>Body</Text>
      </Screen>,
      { scheme },
    );
    expect(screen.toJSON()).toMatchSnapshot();
  });

  it('paints the canvas colour', async () => {
    await renderThemed(
      <Screen testID="screen">
        <Text>Body</Text>
      </Screen>,
      { scheme },
    );
    expect(screen.getByTestId('screen')).toHaveStyle({ backgroundColor: themes[scheme].bg.canvas });
  });
});

describe('Screen behaviour', () => {
  it('exposes the title as a header', async () => {
    await renderThemed(
      <Screen title="Friends">
        <Text>Body</Text>
      </Screen>,
    );
    expect(screen.getByRole('header', { name: 'Friends' })).toBeOnTheScreen();
  });

  it('renders without scrolling for full-bleed content', async () => {
    await renderThemed(
      <Screen scroll={false}>
        <Text>Canvas</Text>
      </Screen>,
    );
    expect(screen.getByText('Canvas')).toBeOnTheScreen();
  });
});
