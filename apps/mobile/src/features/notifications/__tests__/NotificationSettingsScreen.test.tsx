import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, screen, userEvent, waitFor } from '@testing-library/react-native';

import type { ColorScheme } from '@card-sharing/tokens';

import { t } from '../../../i18n';
import { problem } from '../../../test/fakeAuthServer';
import { DEFAULT_PREFS, FakeNotificationsServer } from '../../../test/fakeNotificationsServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { renderThemed, SCHEMES } from '../../../test/render';
import { AuthProvider } from '../../auth/AuthProvider';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import { createNotificationsApi } from '../notificationsApi';
import { NotificationSettingsScreen } from '../NotificationSettingsScreen';

const PATH = '/v1/me/notification-preferences';

async function setup({
  scheme = 'light',
  prepare,
}: { scheme?: ColorScheme; prepare?: (server: FakeNotificationsServer) => void } = {}) {
  const server = new FakeNotificationsServer();
  prepare?.(server);
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: memoryStore(sessionFromTokenPair(server.auth.issue())),
    fetch: server.fetch,
    now: server.now,
  });
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: Infinity },
      mutations: { retry: 0, gcTime: Infinity },
    },
  });
  await renderThemed(
    <QueryClientProvider client={queryClient}>
      <AuthProvider client={client}>
        <NotificationSettingsScreen
          api={createNotificationsApi(client.fetch)}
          onBack={() => undefined}
          timeZone={() => 'Europe/Paris'}
        />
      </AuthProvider>
    </QueryClientProvider>,
    { scheme },
  );
  return { server, user: userEvent.setup() };
}

const saveButton = () => screen.getByRole('button', { name: t('notifications.save') });

describe('NotificationSettingsScreen', () => {
  it.each(SCHEMES)(
    'shows the four alert types and the default quiet hours (%s)',
    async (scheme) => {
      await setup({ scheme });
      for (const type of ['cardNew', 'reaction', 'friend', 'cardReview']) {
        expect((await screen.findByTestId(`notification-type-${type}`)).props.value).toBe(true);
      }
      expect(screen.getByTestId('quiet-hours-switch').props.value).toBe(true);
      expect(screen.getByTestId('quiet-start').props.accessibilityValue).toEqual({ text: '22:00' });
      expect(screen.getByTestId('quiet-end').props.accessibilityValue).toEqual({ text: '08:00' });
      expect(
        screen.getByText(t('notifications.quiet.zone', { zone: 'Europe/Paris' })),
      ).toBeTruthy();
      expect(saveButton()).toBeDisabled();
    },
  );

  it('saves the changed toggles and times with the device time zone in one request', async () => {
    const { server, user } = await setup();
    await fireEvent(await screen.findByTestId('notification-type-reaction'), 'valueChange', false);
    await user.press(
      screen.getByRole('button', {
        name: t('notifications.quiet.time.later', { label: t('notifications.quiet.start') }),
      }),
    );
    await fireEvent(screen.getByTestId('quiet-end'), 'accessibilityAction', {
      nativeEvent: { actionName: 'decrement' },
    });
    await user.press(saveButton());
    await screen.findByText(t('notifications.saved'));
    expect(server.count('PUT', PATH)).toBe(1);
    expect(server.prefs).toEqual({
      types: { ...DEFAULT_PREFS.types, reaction: false },
      quiet_hours: { enabled: true, start: '22:30', end: '07:30' },
      time_zone: 'Europe/Paris',
    });
    await waitFor(() => expect(saveButton()).toBeDisabled());
  });

  it('turns quiet hours off and hides the times', async () => {
    const { server, user } = await setup();
    await fireEvent(await screen.findByTestId('quiet-hours-switch'), 'valueChange', false);
    expect(screen.queryByTestId('quiet-start')).toBeNull();
    await user.press(saveButton());
    await screen.findByText(t('notifications.saved'));
    expect(server.prefs.quiet_hours.enabled).toBe(false);
  });

  it('blocks an empty quiet window before sending it', async () => {
    const { server } = await setup({
      prepare: (s) => {
        s.prefs = { ...s.prefs, quiet_hours: { enabled: true, start: '22:00', end: '22:30' } };
      },
    });
    await fireEvent(await screen.findByTestId('quiet-end'), 'accessibilityAction', {
      nativeEvent: { actionName: 'decrement' },
    });
    expect(screen.getByTestId('quiet-empty')).toBeTruthy();
    expect(saveButton()).toBeDisabled();
    expect(server.count('PUT', PATH)).toBe(0);
  });

  it('keeps the draft and says so when the save is throttled', async () => {
    const { server, user } = await setup();
    await fireEvent(await screen.findByTestId('notification-type-friend'), 'valueChange', false);
    server.overrides.set(`PUT ${PATH}`, problem(429, 'RATE_LIMITED'));
    await user.press(saveButton());
    await screen.findByText(t('notifications.error.rateLimited'));
    expect(screen.getByTestId('notification-type-friend').props.value).toBe(false);
    expect(server.prefs.types.friend).toBe(true);
    expect(saveButton()).toBeEnabled();
  });

  it('offers a retry when the settings cannot load', async () => {
    const { user } = await setup({
      prepare: (s) => s.overrides.set(`GET ${PATH}`, problem(503, 'TEMPORARILY_UNAVAILABLE')),
    });
    await screen.findByText(t('notifications.error.load'));
    await user.press(screen.getByRole('button', { name: t('notifications.retry') }));
    await screen.findByTestId('notification-type-cardNew');
  });
});
