import { screen, userEvent } from '@testing-library/react-native';

import { themes } from '@card-sharing/tokens';

import { renderThemed, SCHEMES } from '../../test/render';
import { Text } from '../Text';
import { Sheet } from '../Sheet';

const body = <Text>Body</Text>;

describe.each(SCHEMES)('Sheet (%s)', (scheme) => {
  it('matches the snapshot when open', async () => {
    await renderThemed(
      <Sheet visible title="Edit caption" onClose={() => {}} testID="sheet">
        {body}
      </Sheet>,
      { scheme },
    );
    expect(screen.toJSON()).toMatchSnapshot();
  });

  it('sits on paper with the sheet elevation', async () => {
    await renderThemed(
      <Sheet visible title="Edit" onClose={() => {}} testID="sheet">
        {body}
      </Sheet>,
      { scheme },
    );
    expect(screen.getByTestId('sheet')).toHaveStyle({ backgroundColor: themes[scheme].bg.paper });
  });
});

describe('Sheet behaviour', () => {
  it('renders nothing when closed', async () => {
    await renderThemed(
      <Sheet visible={false} title="Edit" onClose={() => {}}>
        {body}
      </Sheet>,
    );
    expect(screen.queryByText('Edit')).toBeNull();
  });

  it('announces the title as a header and traps screen-reader focus', async () => {
    await renderThemed(
      <Sheet visible title="Edit" onClose={() => {}} testID="sheet">
        {body}
      </Sheet>,
    );
    expect(screen.getByRole('header', { name: 'Edit' })).toBeOnTheScreen();
    expect(screen.getByTestId('sheet').props.accessibilityViewIsModal).toBe(true);
  });

  it('closes from the close button and from the labelled backdrop', async () => {
    const onClose = jest.fn();
    const user = userEvent.setup();
    await renderThemed(
      <Sheet visible title="Edit" onClose={onClose}>
        {body}
      </Sheet>,
    );
    await user.press(screen.getByRole('button', { name: 'Close' }));
    await user.press(
      screen.getByRole('button', { name: 'Close sheet', includeHiddenElements: true }),
    );
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it.each([
    [false, 'slide'],
    [true, 'fade'],
  ])('reduced motion %s uses the %s animation', async (reducedMotion, animation) => {
    await renderThemed(
      <Sheet visible title="Edit" onClose={() => {}} testID="sheet">
        {body}
      </Sheet>,
      { reducedMotion },
    );
    const modal = screen.getByTestId('sheet-modal', { includeHiddenElements: true });
    expect(modal.props.animationType).toBe(animation);
  });
});
