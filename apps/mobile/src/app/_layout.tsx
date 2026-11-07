import { QueryClientProvider } from '@tanstack/react-query';
import { useState } from 'react';
import { StyleSheet, View } from 'react-native';
import { GestureHandlerRootView } from 'react-native-gesture-handler';
import { SafeAreaProvider } from 'react-native-safe-area-context';

import { DEFAULT_SCHEME, themes } from '@card-sharing/tokens';

import { createQueryClient } from '../data/queryClient';
import { AuthProvider, createAppAuthClient, useAuthClient } from '../features/auth';
import { deviceFeedback, FeedbackProvider, securePrefsStorage } from '../features/feedback';
import { clearDeviceBoards } from '../features/boards';
import { deviceFeedCache, deviceSeenStore, PurgeSync } from '../features/feed';
import { TimeZoneSync, useNotificationsApi } from '../features/notifications';
import { createAppOutbox, deviceOutboxStore, OutboxProvider } from '../features/outbox';
import {
  captureLaunchLink,
  createAppOnboardingDeps,
  ErrorScreen,
  GuardedStack,
  IntendedLinkProvider,
  LoadingScreen,
  OnboardingProvider,
  useIntendedLinkState,
  useMagicLinkLanding,
  useOnboarding,
} from '../features/onboarding';
import { deviceExportStore } from '../features/privacy';
import { RealtimeEvents } from '../features/realtime';
import { useCardsApi } from '../features/sharing';
import { createWidgetTokenApi, deviceWidgetTokenStore, WidgetTokenSync } from '../features/widget';
import { deviceSyncedTimeZoneStore } from '../features/notifications/syncedTimeZone';
import { ThemeProvider, ToastProvider, useBrandFonts, useTheme } from '../ui';

// Web: a magic link's token is read from the URL fragment before the router renders, then removed from the address
// bar (ADR-0012). A no-op elsewhere.
captureLaunchLink();

/** Purge instructions for cached friends' cards (T-83), checked only while signed in. */
function CardPurges() {
  const api = useCardsApi();
  return <PurgeSync api={api} cache={deviceFeedCache()} />;
}

/** Sends the device time zone so quiet hours apply locally (T-87), only while signed in. */
function DeviceTimeZone() {
  const api = useNotificationsApi();
  return <TimeZoneSync api={api} />;
}

/** Keeps a widget token for this session in the widget-token store (T-91), only while signed in. */
function WidgetToken() {
  const authClient = useAuthClient();
  const [api] = useState(() => createWidgetTokenApi(authClient.fetch));
  return <WidgetTokenSync api={api} store={deviceWidgetTokenStore()} />;
}

function PhaseStack() {
  const theme = useTheme();
  const { phase } = useOnboarding();
  // Before any early return: the link the app was opened at must be read while the phase is still loading.
  const { returnTo, pending } = useIntendedLinkState(phase);
  const magicLinkLanding = useMagicLinkLanding(phase);
  if (phase === 'loading') return <LoadingScreen />;
  if (phase === 'error') return <ErrorScreen />;
  return (
    <>
      {phase === 'home' ? (
        <>
          <CardPurges />
          <RealtimeEvents />
          <DeviceTimeZone />
          <WidgetToken />
        </>
      ) : null}
      <IntendedLinkProvider value={pending}>
        <GuardedStack
          phase={phase}
          returnTo={returnTo}
          magicLinkLanding={magicLinkLanding}
          contentStyle={{ backgroundColor: theme.color.bg.canvas }}
        />
      </IntendedLinkProvider>
    </>
  );
}

function Onboarding() {
  const authClient = useAuthClient();
  const [deps] = useState(() => createAppOnboardingDeps(authClient));
  const [outbox] = useState(() => createAppOutbox(authClient));
  return (
    <OnboardingProvider {...deps}>
      <OutboxProvider outbox={outbox}>
        <PhaseStack />
      </OutboxProvider>
    </OnboardingProvider>
  );
}

/**
 * Device copies of account data never outlive the session: the offline feed holds friends' names and captions
 * (T-80), the outbox holds this account's unsent cards and reactions (T-78), which must never go out as the
 * next account, the widget token (T-91) must leave the widget empty (AC-16.2), and board copies (T-105) hold
 * friends' cards and names. The remembered data export (T-155), the seen marks of story rings and the time zone last
 * sent to the server (B7) belong to the account too.
 */
const clearDeviceData = () => {
  void deviceFeedCache()
    .clear()
    .catch(() => undefined);
  void deviceOutboxStore()
    .clear()
    .catch(() => undefined);
  void deviceWidgetTokenStore()
    .clear()
    .catch(() => undefined);
  void clearDeviceBoards().catch(() => undefined);
  void deviceExportStore()
    .clear()
    .catch(() => undefined);
  // Which friends' cards were opened (story rings, sprint 9).
  void deviceSeenStore()
    .clear()
    .catch(() => undefined);
  // The time zone last sent for this account (B7): the next account syncs its own.
  void deviceSyncedTimeZoneStore()
    .clear()
    .catch(() => undefined);
};

export default function RootLayout() {
  const [queryClient] = useState(createQueryClient);
  const [authClient] = useState(createAppAuthClient);
  const [prefsStorage] = useState(securePrefsStorage);
  const [engines] = useState(deviceFeedback);
  // Hold the first frame (at most BRAND_FONTS_TIMEOUT_MS) so screens open in the display face, not a font swap.
  const fontsReady = useBrandFonts();
  if (!fontsReady) return <View style={[styles.root, styles.boot]} />;
  return (
    <GestureHandlerRootView style={styles.root}>
      <SafeAreaProvider>
        <QueryClientProvider client={queryClient}>
          <AuthProvider client={authClient} onSignedOut={clearDeviceData}>
            <ThemeProvider>
              <FeedbackProvider
                storage={prefsStorage}
                haptics={engines.haptics}
                sound={engines.sound}
              >
                <ToastProvider>
                  <Onboarding />
                </ToastProvider>
              </FeedbackProvider>
            </ThemeProvider>
          </AuthProvider>
        </QueryClientProvider>
      </SafeAreaProvider>
    </GestureHandlerRootView>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1 },
  boot: { backgroundColor: themes[DEFAULT_SCHEME].bg.canvas },
});
