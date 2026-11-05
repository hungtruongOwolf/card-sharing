import { QueryClientProvider } from '@tanstack/react-query';
import { Stack } from 'expo-router';
import { useState } from 'react';
import { StyleSheet } from 'react-native';
import { GestureHandlerRootView } from 'react-native-gesture-handler';
import { SafeAreaProvider } from 'react-native-safe-area-context';

import { createQueryClient } from '../data/queryClient';
import { AuthProvider, createAppAuthClient } from '../features/auth';
import { ThemeProvider, ToastProvider, useTheme } from '../ui';

function ThemedStack() {
  const theme = useTheme();
  return (
    <Stack
      screenOptions={{
        headerShown: false,
        contentStyle: { backgroundColor: theme.color.bg.canvas },
      }}
    />
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
                <ThemedStack />
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
