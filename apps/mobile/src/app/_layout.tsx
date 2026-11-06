import { QueryClientProvider } from '@tanstack/react-query';
import { Stack } from 'expo-router';
import { useState } from 'react';
import { StyleSheet } from 'react-native';
import { GestureHandlerRootView } from 'react-native-gesture-handler';
import { SafeAreaProvider } from 'react-native-safe-area-context';

import { createQueryClient } from '../data/queryClient';
import { AuthProvider, createAppAuthClient, useAuthClient } from '../features/auth';
import { FeedbackProvider, securePrefsStorage } from '../features/feedback';
import { clearDeviceBoards } from '../features/boards';
import { deviceFeedCache, PurgeSync } from '../features/feed';
import { TimeZoneSync, useNotificationsApi } from '../features/notifications';
import { createAppOutbox, deviceOutboxStore, OutboxProvider } from '../features/outbox';
import {
  createAppOnboardingDeps,
  ErrorScreen,
  LoadingScreen,
  OnboardingProvider,
  useOnboarding,
  type OnboardingPhase,
} from '../features/onboarding';
import { useCardsApi } from '../features/sharing';
import { createWidgetTokenApi, deviceWidgetTokenStore, WidgetTokenSync } from '../features/widget';
import { ThemeProvider, ToastProvider, useTheme } from '../ui';

/**
 * Routes of each phase. Only the current phase's routes are reachable (src/features/onboarding/phase.ts);
 * the first one listed is where the phase lands.
 */
const ROUTES: readonly (readonly [Exclude<OnboardingPhase, 'loading' | 'error'>, string])[] = [
  ['home', 'index'],
  ['home', 'holo-lab'],
  ['home', 'capture'],
  ['home', 'editor'],
  ['home', 'drafts'],
  ['home', 'send'],
  ['home', 'feed'],
  ['home', 'activity'],
  ['home', 'card/[id]'],
  ['home', 'boards'],
  ['home', 'board/[id]'],
  ['home', 'board-lab'],
  ['home', 'outbox'],
  ['home', 'cut-lab'],
  ['home', 'effects-lab'],
  ['home', 'settings/sessions'],
  ['home', 'settings/privacy'],
  ['home', 'settings/feedback'],
  ['home', 'settings/notifications'],
  ['home', 'friends/index'],
  ['home', 'friends/requests'],
  ['home', 'friends/invite'],
  ['home', 'friends/find'],
  ['home', 'i/[token]'],
  ['signIn', 'onboarding/sign-in'],
  ['refused', 'onboarding/refused'],
  ['birthDate', 'onboarding/birth-date'],
  ['displayName', 'onboarding/display-name'],
  ['handle', 'onboarding/handle'],
  ['camera', 'onboarding/camera'],
];

/**
 * The stack follows the onboarding phase: when the phase changes, the guard of the current screen turns
 * false and Expo Router moves to the only screen left available and drops the old one from history, so
 * there is no "back" into a finished step (the DOB screen in particular, AC-2.3). The server enforces the
 * same order; these guards only keep the UI consistent with it.
 */
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
  if (phase === 'loading') return <LoadingScreen />;
  if (phase === 'error') return <ErrorScreen />;
  return (
    <>
      {phase === 'home' ? (
        <>
          <CardPurges />
          <DeviceTimeZone />
          <WidgetToken />
        </>
      ) : null}
      <Stack
        screenOptions={{
          headerShown: false,
          gestureEnabled: false,
          contentStyle: { backgroundColor: theme.color.bg.canvas },
        }}
      >
        {ROUTES.map(([routePhase, name]) => (
          <Stack.Protected key={name} guard={phase === routePhase}>
            <Stack.Screen name={name} />
          </Stack.Protected>
        ))}
      </Stack>
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
 * friends' cards and names.
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
};

export default function RootLayout() {
  const [queryClient] = useState(createQueryClient);
  const [authClient] = useState(createAppAuthClient);
  const [prefsStorage] = useState(securePrefsStorage);
  return (
    <GestureHandlerRootView style={styles.root}>
      <SafeAreaProvider>
        <QueryClientProvider client={queryClient}>
          <AuthProvider client={authClient} onSignedOut={clearDeviceData}>
            <ThemeProvider>
              <FeedbackProvider storage={prefsStorage}>
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
});
