import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { AccessibilityInfo, Pressable, StyleSheet, View } from 'react-native';
import Animated, { FadeIn, FadeInDown, FadeOut, FadeOutDown } from 'react-native-reanimated';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { motion, radius, space, z } from '@card-sharing/tokens';

import { t } from '../i18n';
import { Button } from './Button';
import { Text } from './Text';
import { useTheme, type Theme } from './theme/ThemeProvider';

export type ToastVariant = 'info' | 'success' | 'error';

export interface ToastOptions {
  /** Message from `t()`. */
  message: string;
  variant?: ToastVariant;
  /** Optional action, e.g. Undo. Keeps the toast up longer. */
  action?: { label: string; onPress: () => void };
}

/** Visible time (design-system.md §4): 4 s, 8 s with an action. */
export const TOAST_DURATION = { plain: 4000, withAction: 8000 } as const;

interface ToastApi {
  show: (options: ToastOptions) => void;
  hide: () => void;
}

const ToastContext = createContext<ToastApi | null>(null);

interface ActiveToast extends ToastOptions {
  id: number;
}

const a11yKey = {
  info: 'toast.a11y.info',
  success: 'toast.a11y.success',
  error: 'toast.a11y.error',
} as const;

function markerColor(theme: Theme, variant: ToastVariant): string {
  const c = theme.color;
  return variant === 'success' ? c.accent.mint : variant === 'error' ? c.danger.base : c.accent.sky;
}

/**
 * Hosts one toast at a time (a new one replaces the current one) above the bottom safe area.
 * Each toast is announced to VoiceOver/TalkBack with its variant ("Error: …"), so colour is never the
 * only signal. Swipe-to-dismiss comes with the gesture work; tap dismisses for now.
 */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [toast, setToast] = useState<ActiveToast | null>(null);
  const nextId = useRef(1);

  const hide = useCallback(() => setToast(null), []);
  const show = useCallback((options: ToastOptions) => {
    const variant = options.variant ?? 'info';
    setToast({ ...options, variant, id: nextId.current++ });
    AccessibilityInfo.announceForAccessibility(t(a11yKey[variant], { message: options.message }));
  }, []);

  useEffect(() => {
    if (!toast) return;
    const ms = toast.action ? TOAST_DURATION.withAction : TOAST_DURATION.plain;
    const timer = setTimeout(() => setToast((cur) => (cur?.id === toast.id ? null : cur)), ms);
    return () => clearTimeout(timer);
  }, [toast]);

  const api = useMemo(() => ({ show, hide }), [show, hide]);

  return (
    <ToastContext.Provider value={api}>
      {children}
      {toast ? <ToastView key={toast.id} toast={toast} onDismiss={hide} /> : null}
    </ToastContext.Provider>
  );
}

function ToastView({ toast, onDismiss }: { toast: ActiveToast; onDismiss: () => void }) {
  const theme = useTheme();
  const insets = useSafeAreaInsets();
  const variant = toast.variant ?? 'info';
  const reduced = theme.reducedMotion;
  const entering = reduced
    ? FadeIn.duration(motion.reducedDuration)
    : FadeInDown.duration(motion.duration.md);
  const exiting = reduced
    ? FadeOut.duration(motion.reducedDuration)
    : FadeOutDown.duration(motion.duration.sm);

  return (
    <View
      pointerEvents="box-none"
      style={[styles.host, { paddingBottom: Math.max(insets.bottom, space[4]) }]}
    >
      <Animated.View
        testID="toast"
        entering={entering}
        exiting={exiting}
        style={[
          styles.toast,
          { backgroundColor: theme.color.surface.raised, boxShadow: theme.shadow(3) },
        ]}
      >
        <View
          testID="toast-marker"
          style={[styles.marker, { backgroundColor: markerColor(theme, variant) }]}
        />
        <Pressable
          style={[styles.body, { minHeight: theme.touchMin }]}
          onPress={onDismiss}
          accessibilityLabel={t(a11yKey[variant], { message: toast.message })}
          accessibilityHint={t('toast.dismiss.hint')}
        >
          <Text variant="body.md">{toast.message}</Text>
        </Pressable>
        {toast.action ? (
          <Button
            variant="ghost"
            label={toast.action.label}
            onPress={() => {
              toast.action?.onPress();
              onDismiss();
            }}
          />
        ) : null}
      </Animated.View>
    </View>
  );
}

export function useToast(): ToastApi {
  const api = useContext(ToastContext);
  if (!api) throw new Error('useToast must be used inside <ToastProvider>');
  return api;
}

const styles = StyleSheet.create({
  host: {
    ...StyleSheet.absoluteFill,
    justifyContent: 'flex-end',
    paddingHorizontal: space[4],
    zIndex: z.toast,
  },
  toast: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: space[2],
    borderRadius: radius.lg,
    paddingEnd: space[2],
    overflow: 'hidden',
  },
  marker: {
    alignSelf: 'stretch',
    width: space[1],
  },
  body: { flex: 1, justifyContent: 'center', paddingVertical: space[2], paddingStart: space[2] },
});
