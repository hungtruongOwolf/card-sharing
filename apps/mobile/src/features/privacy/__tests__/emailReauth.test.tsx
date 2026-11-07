import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, screen, userEvent } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { FakePrivacyServer } from '../../../test/fakePrivacyServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { renderThemed } from '../../../test/render';
import { AuthProvider } from '../../auth/AuthProvider';
import { createAuthClient } from '../../auth/authClient';
import {
  appMagicLinkChannel,
  createMagicLinkChannel,
  parseMagicLinkMessage,
  type MagicLinkMessage,
} from '../../auth/magicLinkChannel';
import { sessionFromTokenPair } from '../../auth/session';
import { memoryKeyValueStore } from '../../drafts/draftStore';
import { MagicLinkScreen } from '../../onboarding/screens/MagicLinkScreen';
import { DataExportScreen } from '../DataExportScreen';
import { createExportStore } from '../exportStore';
import { createPrivacyApi } from '../privacyApi';

const RID = '0192f0a1-2b3c-7d4e-8f90-123456789abc';
const TOKEN = 'B'.repeat(43);
const EMAIL = 'cat@example.com';

describe('magic-link channel (ADR-0012)', () => {
  it('lets only well-formed messages through', () => {
    expect(parseMagicLinkMessage({ type: 'link', requestId: RID, token: TOKEN })).toEqual({
      type: 'link',
      requestId: RID,
      token: TOKEN,
    });
    expect(parseMagicLinkMessage({ type: 'ack', requestId: RID })).toEqual({
      type: 'ack',
      requestId: RID,
    });
    expect(parseMagicLinkMessage({ type: 'link', requestId: RID, token: 'short' })).toBeNull();
    expect(parseMagicLinkMessage({ type: 'link', requestId: 'x', token: TOKEN })).toBeNull();
    expect(parseMagicLinkMessage('link')).toBeNull();
  });

  it('delivers to other tabs over BroadcastChannel and to listeners in the same tab', () => {
    const tabs = new Set<FakeBroadcast>();
    class FakeBroadcast {
      listener: ((event: { data: unknown }) => void) | null = null;
      constructor(_name: string) {
        tabs.add(this);
      }
      // Every other tab hears it, never the sender (like BroadcastChannel).
      postMessage(message: unknown) {
        for (const tab of tabs) if (tab !== this) tab.listener?.({ data: message });
      }
      addEventListener(_type: 'message', listener: (event: { data: unknown }) => void) {
        this.listener = listener;
      }
    }
    const asking = createMagicLinkChannel(FakeBroadcast);
    const heardInAskingTab: MagicLinkMessage[] = [];
    asking.subscribe((message) => heardInAskingTab.push(message));
    const landing = createMagicLinkChannel(FakeBroadcast);
    const heardInLandingTab: MagicLinkMessage[] = [];
    landing.subscribe((message) => heardInLandingTab.push(message));

    landing.post({ type: 'link', requestId: RID, token: TOKEN });
    expect(heardInAskingTab).toEqual([{ type: 'link', requestId: RID, token: TOKEN }]);
    expect(heardInLandingTab).toEqual([{ type: 'link', requestId: RID, token: TOKEN }]);
  });
});

async function renderExport(server: FakePrivacyServer) {
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: memoryStore(sessionFromTokenPair(server.auth.issue())),
    fetch: server.fetch,
    now: server.auth.now,
    logoutTimeoutMs: 50,
  });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: 0 } },
  });
  await renderThemed(
    <QueryClientProvider client={queryClient}>
      <AuthProvider client={client}>
        <DataExportScreen
          api={createPrivacyApi(client)}
          providers={[]}
          store={createExportStore(memoryKeyValueStore())}
          baseUrl={() => server.baseUrl}
          openUrl={async () => true}
          onBack={() => undefined}
          now={server.auth.now}
        />
      </AuthProvider>
    </QueryClientProvider>,
  );
  return userEvent.setup();
}

async function askForReauthLink(user: ReturnType<typeof userEvent.setup>, email: string) {
  await user.press(await screen.findByRole('button', { name: t('dataExport.request') }));
  await user.press(await screen.findByRole('button', { name: t('reauth.email') }));
  await user.type(screen.getByTestId('reauth-email'), email);
  await user.press(screen.getByRole('button', { name: t('reauth.emailSend') }));
  await screen.findByTestId('reauth-email-sent');
}

describe('confirm it’s you by email (ADR-0012)', () => {
  it('redeems the link handed over by the tab it opened in, then requests the export', async () => {
    const server = new FakePrivacyServer();
    server.accountEmail = EMAIL;
    const user = await renderExport(server);
    await askForReauthLink(user, 'Cat@Example.com');
    expect(server.auth.count('POST', '/v1/me/exports')).toBe(0);
    const link = server.reauthLinks[0];
    if (!link) throw new Error('no re-auth link sent');

    const acks: string[] = [];
    const off = appMagicLinkChannel().subscribe((m) => {
      if (m.type === 'ack') acks.push(m.requestId);
    });
    await act(async () => {
      appMagicLinkChannel().post({ type: 'link', requestId: link.requestId, token: link.token });
      // A second hand-off of the same link (e.g. the landing tab reloaded) is acknowledged, not redeemed again.
      appMagicLinkChannel().post({ type: 'link', requestId: link.requestId, token: link.token });
    });
    await screen.findByTestId('data-export-status');
    off();
    expect(acks).toEqual([link.requestId, link.requestId]);
    expect(server.auth.count('POST', '/v1/auth/reauth/email/verify')).toBe(1);
    expect(server.auth.count('POST', '/v1/me/exports')).toBe(1);
    // The verifier never left this tab: only its hash went with the request.
    expect(JSON.stringify(server.auth.requests.map((r) => r.body))).toContain('code_verifier');
  });

  it('ignores links of other requests and says so when a link is refused', async () => {
    const server = new FakePrivacyServer();
    server.accountEmail = EMAIL;
    const user = await renderExport(server);
    await askForReauthLink(user, EMAIL);
    const link = server.reauthLinks[0];
    if (!link) throw new Error('no re-auth link sent');
    await act(async () => {
      appMagicLinkChannel().post({ type: 'link', requestId: RID, token: link.token });
    });
    expect(server.auth.count('POST', '/v1/auth/reauth/email/verify')).toBe(0);

    link.used = true;
    await act(async () => {
      appMagicLinkChannel().post({ type: 'link', requestId: link.requestId, token: link.token });
    });
    expect(await screen.findByText(t('reauth.linkSpent'))).toBeTruthy();
    expect(server.auth.count('POST', '/v1/me/exports')).toBe(0);
  });

  it('answers the same for an address that is not the account’s', async () => {
    const server = new FakePrivacyServer();
    server.accountEmail = EMAIL;
    const user = await renderExport(server);
    await askForReauthLink(user, 'someone-else@example.com');
    expect(server.reauthLinks).toEqual([]);
  });
});

describe('re-auth link landing tab (ADR-0012)', () => {
  it('hands the link to the asking tab and says so', async () => {
    const channel = createMagicLinkChannel(undefined);
    const off = channel.subscribe((m) => {
      if (m.type === 'link') channel.post({ type: 'ack', requestId: m.requestId });
    });
    const onDone = jest.fn();
    await renderThemed(
      <MagicLinkScreen
        link={{ requestId: RID, token: TOKEN, purpose: 'reauth' }}
        magicLink={null}
        channel={channel}
        onContinueAt={() => undefined}
        onRestart={() => undefined}
        onDone={onDone}
      />,
    );
    await screen.findByTestId('magic-link-reauthSent');
    await userEvent.setup().press(screen.getByRole('button', { name: t('magicLink.backToApp') }));
    expect(onDone).toHaveBeenCalled();
    off();
  });

  it('says to go back to the asking tab when nobody took the link', async () => {
    await renderThemed(
      <MagicLinkScreen
        link={{ requestId: RID, token: TOKEN, purpose: 'reauth' }}
        magicLink={null}
        channel={createMagicLinkChannel(undefined)}
        onContinueAt={() => undefined}
        onRestart={() => undefined}
      />,
    );
    expect(await screen.findByTestId('magic-link-reauthNoTab', {}, { timeout: 8000 })).toBeTruthy();
  });
});
