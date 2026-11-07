import { screen, userEvent } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { FakeOnboardingServer } from '../../../test/fakeOnboardingServer';
import { memoryKeyValue, renderOnboarding, testRandom } from '../../../test/onboardingHarness';
import { createMagicLinkSignIn, LINK_TTL_MS, type MagicLinkKeyValue } from '../magicLink';

const EMAIL = 'cat@example.com';
const button = (name: string) => screen.getByRole('button', { name });

/**
 * "Tab 1" asked for a link to EMAIL (the sign-in screen itself is covered below): the record is in `store`, the link in
 * the fake server's outbox.
 */
async function askForLink(
  server: FakeOnboardingServer,
  store: MagicLinkKeyValue,
  context: { inviteCode?: string; returnTo?: string } = {},
) {
  const tab1 = createMagicLinkSignIn({
    baseUrl: () => server.baseUrl,
    authClient: { signInWithMagicLink: jest.fn() },
    store,
    fetch: server.fetch,
    now: server.now,
    random: testRandom(),
  });
  const { requestId } = await tab1.start(EMAIL, context);
  return { requestId, token: server.auth.linkFor(EMAIL).token, purpose: 'sign_in' as const };
}

describe('email sign-in, sign-in screen (US-1, ADR-0012)', () => {
  it('asks for a link and says where to open it', async () => {
    const server = new FakeOnboardingServer();
    await renderOnboarding(server, { inviteCode: 'ABCD-EFGH-JKMN' });
    const user = userEvent.setup();
    expect(await screen.findByText(t('onboarding.signIn.inviteReady'))).toBeTruthy();
    await user.type(screen.getByTestId('sign-in-email'), ' Cat@Example.com ');
    await user.press(button(t('onboarding.signIn.emailSend')));
    await screen.findByTestId('check-email');
    expect(
      screen.getByText(t('onboarding.checkEmail.body', { email: 'Cat@Example.com' })),
    ).toBeTruthy();
    expect(server.auth.linkFor(EMAIL).email).toBe(EMAIL);
    // "Use a different email" goes back to the form.
    await user.press(button(t('onboarding.checkEmail.change')));
    expect(screen.getByTestId('email-sign-in')).toBeTruthy();
  });

  it('checks the address before asking the server', async () => {
    const server = new FakeOnboardingServer();
    await renderOnboarding(server);
    const user = userEvent.setup();
    await user.type(await screen.findByTestId('sign-in-email'), 'not-an-email');
    await user.press(button(t('onboarding.signIn.emailSend')));
    expect(await screen.findAllByText(t('onboarding.signIn.emailInvalid'))).not.toHaveLength(0);
    expect(server.auth.count('POST', '/v1/auth/email/start')).toBe(0);
  });

  it('says so when too many links were asked for', async () => {
    const server = new FakeOnboardingServer();
    const tooMany = jest.fn(async () => new Response(null, { status: 429 }));
    await renderOnboarding(server, {
      magicLink: (authClient) =>
        createMagicLinkSignIn({
          baseUrl: () => server.baseUrl,
          authClient,
          store: memoryKeyValue(),
          fetch: tooMany,
          random: testRandom(),
        }),
    });
    const user = userEvent.setup();
    await user.type(await screen.findByTestId('sign-in-email'), EMAIL);
    await user.press(button(t('onboarding.signIn.emailSend')));
    expect(await screen.findByText(t('onboarding.signIn.emailRateLimited'))).toBeTruthy();
  });
});

describe('email sign-in, landing screen (T-23, ADR-0012)', () => {
  it('a new user confirms, gives an invite code after a wrong one, and lands on the age step', async () => {
    const server = new FakeOnboardingServer();
    server.auth.inviteRequired = true;
    server.auth.inviteCodes.set('ABCDEFGHJKMN', 1);
    const store = memoryKeyValue();
    const landing = await askForLink(server, store);

    await renderOnboarding(server, { magicLinkStore: store, landing });
    const user = userEvent.setup();
    // Opening the link signs nobody in: the user confirms first (AC-1.3).
    await screen.findByTestId('magic-link-confirm');
    expect(server.auth.count('POST', '/v1/auth/email/verify')).toBe(0);
    await user.press(button(t('magicLink.continue')));

    await screen.findByTestId('magic-link-invite');
    const field = screen.getByTestId('invite-code');
    await user.type(field, 'ZZZZ-ZZZZ-ZZZZ');
    await user.press(button(t('magicLink.inviteJoin')));
    expect(await screen.findByText(t('magicLink.inviteInvalid'))).toBeTruthy();

    await user.clear(field);
    await user.type(field, 'abcd efgh jkmn');
    await user.press(button(t('magicLink.inviteJoin')));
    await screen.findByTestId('onboarding-birth-date');
    // The record is gone after use.
    expect([...(store as ReturnType<typeof memoryKeyValue>).items.keys()]).toEqual([
      'cs.magiclink.index.v1',
    ]);
  });

  it('sends the invite code from the sign-up link without asking', async () => {
    const server = new FakeOnboardingServer();
    server.auth.inviteRequired = true;
    server.auth.inviteCodes.set('ABCDEFGHJKMN', 1);
    const store = memoryKeyValue();
    const landing = await askForLink(server, store, { inviteCode: 'ABCD-EFGH-JKMN' });
    await renderOnboarding(server, { magicLinkStore: store, landing });
    await userEvent
      .setup()
      .press(await screen.findByRole('button', { name: t('magicLink.continue') }));
    await screen.findByTestId('onboarding-birth-date');
  });

  it('continues at the link the user started from', async () => {
    const server = new FakeOnboardingServer();
    const store = memoryKeyValue();
    const landing = await askForLink(server, store, { returnTo: '/i/abc' });
    const onContinueAt = jest.fn();
    await renderOnboarding(server, {
      magicLinkStore: store,
      landing,
      onContinueAt,
    });
    await userEvent
      .setup()
      .press(await screen.findByRole('button', { name: t('magicLink.continue') }));
    await screen.findByTestId('onboarding-birth-date');
    expect(onContinueAt).toHaveBeenCalledWith('/i/abc');
  });

  it('a link opened in another browser says where to open it', async () => {
    const server = new FakeOnboardingServer();
    const landing = await askForLink(server, memoryKeyValue());
    const onRestart = jest.fn();
    // A fresh store: this browser never asked.
    await renderOnboarding(server, { magicLinkStore: memoryKeyValue(), landing, onRestart });
    await screen.findByTestId('magic-link-otherBrowser');
    expect(screen.getByText(t('magicLink.otherBrowserBody'))).toBeTruthy();
    await userEvent.setup().press(button(t('magicLink.restart')));
    expect(onRestart).toHaveBeenCalled();
    expect(server.auth.count('POST', '/v1/auth/email/verify')).toBe(0);
  });

  it('an expired link says so without asking the server', async () => {
    const server = new FakeOnboardingServer();
    const store = memoryKeyValue();
    const landing = await askForLink(server, store);
    server.auth.advance(LINK_TTL_MS + 1000);
    await renderOnboarding(server, { magicLinkStore: store, landing });
    await screen.findByTestId('magic-link-expired');
    expect(server.auth.count('POST', '/v1/auth/email/verify')).toBe(0);
  });

  it('a link used elsewhere says so', async () => {
    const server = new FakeOnboardingServer();
    const store = memoryKeyValue();
    const landing = await askForLink(server, store);
    server.auth.linkFor(EMAIL).used = true;
    await renderOnboarding(server, { magicLinkStore: store, landing });
    await userEvent
      .setup()
      .press(await screen.findByRole('button', { name: t('magicLink.continue') }));
    await screen.findByTestId('magic-link-used');
  });

  it('an incomplete link says so', async () => {
    const server = new FakeOnboardingServer();
    await renderOnboarding(server, { landing: null });
    await screen.findByTestId('magic-link-badLink');
  });
});
