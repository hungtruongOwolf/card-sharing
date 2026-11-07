import type { ReactNode } from 'react';
import { Modal, Pressable, StyleSheet, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { radius, size, space } from '@card-sharing/tokens';

import { t } from '../i18n';
import { Button } from './Button';
import { Text } from './Text';
import { useTheme } from './theme/ThemeProvider';

export interface SheetProps {
  visible: boolean;
  /** Called on backdrop tap, the close button, Android back and the iOS escape gesture. */
  onClose: () => void;
  /** Sheet title from `t()`; announced as a header. */
  title: string;
  children: ReactNode;
  testID?: string;
}

/**
 * Modal bottom sheet on paper (`bg.paper`, `elev.4`, `radius.xl` top corners, 36×4 grabber).
 * Focus is trapped while open (`accessibilityViewIsModal`); there is always a visible close button so
 * screen-reader and switch users never depend on the backdrop. Slides in, or fades with reduced motion.
 * Drag-to-dismiss and peek/half/full detents come with the gesture work (design-system.md §4).
 */
export function Sheet({ visible, onClose, title, children, testID }: SheetProps) {
  const theme = useTheme();
  const insets = useSafeAreaInsets();
  const c = theme.color;

  return (
    <Modal
      testID={testID ? `${testID}-modal` : undefined}
      visible={visible}
      transparent
      animationType={theme.reducedMotion ? 'fade' : 'slide'}
      onRequestClose={onClose}
      statusBarTranslucent
      navigationBarTranslucent
    >
      <View style={styles.root}>
        {/* Hidden from VoiceOver by accessibilityViewIsModal below; TalkBack users can still reach it. */}
        <Pressable
          testID={testID ? `${testID}-backdrop` : undefined}
          style={[StyleSheet.absoluteFill, { backgroundColor: theme.scrim }]}
          onPress={onClose}
          accessibilityRole="button"
          accessibilityLabel={t('sheet.backdrop.label')}
        />
        <View
          testID={testID}
          accessibilityViewIsModal
          onAccessibilityEscape={onClose}
          style={[
            styles.sheet,
            {
              backgroundColor: c.bg.paper,
              boxShadow: theme.shadow(4),
              paddingBottom: Math.max(insets.bottom, space[4]),
            },
          ]}
        >
          <View
            style={[styles.grabber, { backgroundColor: theme.hairline }]}
            accessibilityElementsHidden
            importantForAccessibility="no-hide-descendants"
          />
          <View style={styles.header}>
            <Text variant="title.md" accessibilityRole="header" style={styles.title}>
              {title}
            </Text>
            <Button variant="ghost" label={t('common.close')} onPress={onClose} />
          </View>
          {children}
        </View>
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1, justifyContent: 'flex-end' },
  sheet: {
    borderTopStartRadius: radius.xl,
    borderTopEndRadius: radius.xl,
    paddingHorizontal: space[4],
    paddingTop: space[2],
    gap: space[3],
    maxHeight: '90%',
  },
  grabber: {
    alignSelf: 'center',
    width: size.grabber.width,
    height: size.grabber.height,
    borderRadius: radius.full,
  },
  header: { flexDirection: 'row', alignItems: 'center', gap: space[2] },
  title: { flex: 1 },
});
