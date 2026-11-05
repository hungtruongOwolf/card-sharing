import { screen, userEvent } from '@testing-library/react-native';

import { themes } from '@card-sharing/tokens';

import { renderThemed, SCHEMES } from '../../test/render';
import { TextField } from '../TextField';

describe.each(SCHEMES)('TextField (%s)', (scheme) => {
  it('matches the snapshot with hint and with error', async () => {
    await renderThemed(
      <>
        <TextField label="Name" hint="Shown to friends" placeholder="Your name" />
        <TextField label="Handle" error="Already taken" />
      </>,
      { scheme },
    );
    expect(screen.toJSON()).toMatchSnapshot();
  });

  it('uses the scheme colours', async () => {
    await renderThemed(<TextField label="Name" error="Too long" />, { scheme });
    expect(screen.getByLabelText('Name')).toHaveStyle({
      color: themes[scheme].fg.ink,
      backgroundColor: themes[scheme].surface.raised,
      borderColor: themes[scheme].danger.base,
    });
    expect(screen.getByText('Too long')).toHaveStyle({ color: themes[scheme].danger.base });
  });
});

describe('TextField behaviour', () => {
  it('is labelled for screen readers and reports typed text', async () => {
    const onChangeText = jest.fn();
    const user = userEvent.setup();
    await renderThemed(<TextField label="Caption" onChangeText={onChangeText} />);
    await user.type(screen.getByLabelText('Caption'), 'hi');
    expect(onChangeText).toHaveBeenLastCalledWith('hi');
  });

  it('reads the hint, or the error prefixed with "Error", as the accessibility hint', async () => {
    const { rerender } = await renderThemed(<TextField label="Caption" hint="Up to 80" />);
    expect(screen.getByLabelText('Caption').props.accessibilityHint).toBe('Up to 80');
    await rerender(<TextField label="Caption" hint="Up to 80" error="Too long" />);
    expect(screen.getByLabelText('Caption').props.accessibilityHint).toBe('Error: Too long');
    expect(screen.getByRole('alert')).toHaveTextContent('Too long');
  });

  it('meets the touch target and scales text to 200 %', async () => {
    await renderThemed(<TextField label="Caption" />);
    const input = screen.getByLabelText('Caption');
    expect(input).toHaveStyle({ minHeight: 48 });
    expect(input.props.maxFontSizeMultiplier).toBe(2);
  });

  it('reports the disabled state', async () => {
    await renderThemed(<TextField label="Caption" editable={false} />);
    expect(screen.getByLabelText('Caption')).toBeDisabled();
  });
});
