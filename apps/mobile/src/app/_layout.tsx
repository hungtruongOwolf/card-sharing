import { QueryClientProvider } from '@tanstack/react-query';
import { Stack } from 'expo-router';
import { useState } from 'react';
import { StyleSheet } from 'react-native';
import { GestureHandlerRootView } from 'react-native-gesture-handler';
import { SafeAreaProvider } from 'react-native-safe-area-context';

import { createQueryClient } from '../data/queryClient';
import { AuthProvider, createAppAuthClient, useAuthClient } from '../features/auth';
import {
  createAppOnboardingDeps,
  ErrorScreen,
  LoadingScreen,
  OnboardingProvider,
  useOnboarding,
  type OnboardingPhase,
} from '../features/onboarding';
import { ThemeProvider, ToastProvider, useTheme } from '../ui';

/**
 * Routes of each phase. Only the current phase's routes are reachable (src/features/onboarding/phase.ts);
 * the first one listed is where the phase lands.
 */
const ROUTES: readonly (readonly [Exclude<OnboardingPhase, 'loading' | 'error'>, string])[] = [
  ['home', 'index'],
  ['home', 'holo-lab'],
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
function PhaseStack() {
  const theme = useTheme();
  const { phase } = useOnboarding();
  if (phase === 'loading') return <LoadingScreen />;
  if (phase === 'error') return <ErrorScreen />;
  return (
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
  );
}

function Onboarding() {
  const authClient = useAuthClient();
  const [deps] = useState(() => createAppOnboardingDeps(authClient));
  return (
    <OnboardingProvider {...deps}>
      <PhaseStack />
    </OnboardingProvider>
  );
}

export default function RootLayout() {
  const [queryClient] = useState(createQueryClient);
  const [authClient] = useState(createAppAuthClient);
  return (
    <GestureHandlerRootView style={styles.root}>
      <SafeAreaProvider>
        <QueryClientProvider client={queryClient}>
          <AuthProvider client={authClient}>
            <ThemeProvider>
              <ToastProvider>
                <Onboarding />
              </ToastProvider>
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
