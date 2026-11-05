import { act, fireEvent, screen } from '@testing-library/react-native';
import { AccessibilityInfo } from 'react-native';

import { themes } from '@card-sharing/tokens';

import { renderThemed, SCHEMES } from '../../test/render';
import { Button } from '../Button';
import { TOAST_DURATION, useToast, type ToastOptions } from '../Toast';

function Trigger({ options }: { options: ToastOptions }) {
  const toast = useToast();
  return <Button label="Show" onPress={() => toast.show(options)} />;
}

async function showToast(options: ToastOptions, scheme: 'light' | 'dark' = 'light') {
  await renderThemed(<Trigger options={options} />, { scheme });
  await fireEvent.press(screen.getByRole('button', { name: 'Show' }));
}

beforeEach(() => {
  jest.useFakeTimers();
  jest.spyOn(AccessibilityInfo, 'announceForAccessibility').mockImplementation(() => {});
});
afterEach(() => {
  jest.useRealTimers();
  jest.restoreAllMocks();
});

describe.each(SCHEMES)('Toast (%s)', (scheme) => {
  it('matches the snapshot with an action', async () => {
    await showToast(
      { message: 'Caption saved', action: { label: 'Undo', onPress: () => {} } },
      scheme,
    );
    expect(screen.toJSON()).toMatchSnapshot();
  });

  it('uses the raised surface and a variant marker', async () => {
    await showToast({ message: 'Oops', variant: 'error' }, scheme);
    expect(screen.getByTestId('toast')).toHaveStyle({
      backgroundColor: themes[scheme].surface.raised,
    });
    expect(screen.getByTestId('toast-marker')).toHaveStyle({
      backgroundColor: themes[scheme].danger.base,
    });
  });
});

describe('Toast behaviour', () => {
  it('announces the message with its variant so colour is not the only signal', async () => {
    await showToast({ message: 'Upload failed', variant: 'error' });
    expect(AccessibilityInfo.announceForAccessibility).toHaveBeenCalledWith('Error: Upload failed');
    expect(screen.getByLabelText('Error: Upload failed')).toBeOnTheScreen();
  });

  it('hides after 4 s', async () => {
    await showToast({ message: 'Sent' });
    expect(screen.getByText('Sent')).toBeOnTheScreen();
    await act(async () => {
      jest.advanceTimersByTime(TOAST_DURATION.plain);
    });
    expect(screen.queryByText('Sent')).toBeNull();
  });

  it('stays 8 s with an action and runs the action once', async () => {
    const onUndo = jest.fn();
    await showToast({ message: 'Deleted', action: { label: 'Undo', onPress: onUndo } });
    await act(async () => {
      jest.advanceTimersByTime(TOAST_DURATION.plain);
    });
    expect(screen.getByText('Deleted')).toBeOnTheScreen();
    await fireEvent.press(screen.getByRole('button', { name: 'Undo' }));
    expect(onUndo).toHaveBeenCalledTimes(1);
    expect(screen.queryByText('Deleted')).toBeNull();
  });

  it('shows one toast at a time and dismisses on tap', async () => {
    await renderThemed(
      <>
        <Trigger options={{ message: 'First' }} />
        <Trigger options={{ message: 'Second' }} />
      </>,
    );
    const [first, second] = screen.getAllByRole('button', { name: 'Show' });
    await fireEvent.press(first!);
    await fireEvent.press(second!);
    expect(screen.queryByText('First')).toBeNull();
    await fireEvent.press(screen.getByText('Second'));
    expect(screen.queryByText('Second')).toBeNull();
  });
});
