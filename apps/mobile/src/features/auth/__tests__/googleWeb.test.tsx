import { act, screen, userEvent } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { FakeOnboardingServer } from '../../../test/fakeOnboardingServer';
import { renderOnboarding, testRandom } from '../../../test/onboardingHarness';
import { renderThemed } from '../../../test/render';
import { GoogleWebButton, type GoogleCredential } from '../GoogleWebButton';
import { parseGoogleWebClientId, type GisApi } from '../googleIdentity';
import { asciiBytes, hex, sha256 } from '../pkce';

const CLIENT_ID = '123456789012-abcdef0123456789.apps.googleusercontent.com';

/** A stand-in for `google.accounts.id` that records what the app configured. */
function fakeGis() {
  const calls: { config?: Parameters<GisApi['initialize']>[0]; rendered: unknown[] } = {
    rendered: [],
  };
  const gis: GisApi = {
    initialize: (config) => {
      calls.config = config;
    },
    renderButton: (parent, options) => {
      calls.rendered.push({ parent, options });
    },
  };
  /** What Google calls after the user picked an account. */
  const answer = (credential: string) =>
    act(async () => {
      calls.config?.callback({ credential });
    });
  return { gis, calls, answer, loadGis: async () => gis };
}

describe('Google web client id (ADR-0012)', () => {
  it('accepts an OAuth client id only', () => {
    expect(parseGoogleWebClientId(` ${CLIENT_ID} `)).toBe(CLIENT_ID);
    expect(parseGoogleWebClientId(undefined)).toBeNull();
    expect(parseGoogleWebClientId('')).toBeNull();
    expect(parseGoogleWebClientId('my-secret')).toBeNull();
  });
});

describe('GoogleWebButton', () => {
  it('renders Google’s button with the SHA-256 of a fresh nonce and hands back the token with the raw nonce', async () => {
    const fake = fakeGis();
    const credentials: GoogleCredential[] = [];
    await renderThemed(
      <GoogleWebButton
        clientId={CLIENT_ID}
        context="signin"
        loadGis={fake.loadGis}
        random={testRandom()}
        onCredential={(credential) => credentials.push(credential)}
      />,
    );
    await act(async () => undefined);
    expect(fake.calls.config).toMatchObject({
      client_id: CLIENT_ID,
      ux_mode: 'popup',
      context: 'signin',
    });
    expect(fake.calls.rendered).toHaveLength(1);
    await fake.answer('id-token-from-google');
    expect(credentials).toHaveLength(1);
    const [credential] = credentials;
    expect(credential?.idToken).toBe('id-token-from-google');
    expect(credential?.nonce).toMatch(/^[A-Za-z0-9_-]{43}$/);
    expect(fake.calls.config?.nonce).toBe(hex(sha256(asciiBytes(credential?.nonce ?? ''))));
  });

  it('says when Google’s script cannot load', async () => {
    await renderThemed(
      <GoogleWebButton
        clientId={CLIENT_ID}
        context="use"
        loadGis={() => Promise.reject(new Error('blocked'))}
        random={testRandom()}
        onCredential={() => undefined}
      />,
    );
    expect(await screen.findByText(t('googleWeb.unavailable'))).toBeTruthy();
  });
});

describe('Google sign-in on the sign-in screen', () => {
  it('asks for an invite code when the account is new, then retries the same credential', async () => {
    const server = new FakeOnboardingServer();
    server.auth.inviteRequired = true;
    server.auth.inviteCodes.set('ABCDEFGHJKMN', 1);
    const fake = fakeGis();
    await renderOnboarding(server, { googleWeb: { clientId: CLIENT_ID, loadGis: fake.loadGis } });
    await screen.findByTestId('google-web-button');
    await act(async () => undefined);
    await fake.answer('valid-id-token');

    const field = await screen.findByTestId('google-invite-code');
    const user = userEvent.setup();
    await user.type(field, 'ABCD-EFGH-JKMN');
    await user.press(screen.getByRole('button', { name: t('magicLink.inviteJoin') }));
    await screen.findByTestId('onboarding-birth-date');

    const signIns = server.auth.requests.filter((r) => r.path === '/v1/auth/google');
    expect(signIns).toHaveLength(2);
    const [first, second] = signIns.map((r) => r.body as Record<string, unknown>);
    // The same token and nonce, now with the code.
    expect(second?.id_token).toBe(first?.id_token);
    expect(second?.nonce).toBe(first?.nonce);
    expect(first?.invite_code).toBeUndefined();
    expect(second?.invite_code).toBe('ABCD-EFGH-JKMN');
  });

  it('is hidden without a client id', async () => {
    await renderOnboarding(new FakeOnboardingServer());
    await screen.findByTestId('onboarding-sign-in');
    expect(screen.queryByTestId('google-web-button')).toBeNull();
  });
});
