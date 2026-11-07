import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { screen, userEvent } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { renderThemed } from '../../../test/render';
import { SignOutButton } from '../SignOutButton';

async function renderButton(signOut: () => Promise<void>) {
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: 0 } } });
  await renderThemed(
    <QueryClientProvider client={queryClient}>
      <SignOutButton signOut={signOut} />
    </QueryClientProvider>,
  );
}

describe('SignOutButton (B8)', () => {
  it('signs this device out once per press', async () => {
    const signOut = jest.fn(async () => undefined);
    await renderButton(signOut);
    await userEvent.setup().press(screen.getByRole('button', { name: t('sessions.signOutHere') }));
    expect(signOut).toHaveBeenCalledTimes(1);
  });

  it('says so when signing out fails', async () => {
    await renderButton(async () => {
      throw new Error('storage unavailable');
    });
    await userEvent.setup().press(screen.getByRole('button', { name: t('sessions.signOutHere') }));
    expect(await screen.findByText(t('sessions.signOutHere.failed'))).toBeTruthy();
  });
});
