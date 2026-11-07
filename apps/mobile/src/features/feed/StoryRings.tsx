import { Pressable, ScrollView, StyleSheet, View } from 'react-native';
import Animated, { FadeInRight } from 'react-native-reanimated';

import { motion, size, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Avatar, Text, useTheme } from '../../ui';
import type { StoryRing } from './stories';

export interface StoryRingsProps {
  rings: readonly StoryRing[];
  onOpen: (cardId: string) => void;
}

/**
 * The story row at the top of the feed: one ring per friend who sent cards, gradient when something is new. A tap
 * opens the story viewer at their oldest new card. Rings slide in with a short stagger (none with reduced motion).
 */
export function StoryRings({ rings, onOpen }: StoryRingsProps) {
  const theme = useTheme();
  if (rings.length === 0) return null;
  return (
    <ScrollView
      horizontal
      showsHorizontalScrollIndicator={false}
      contentContainerStyle={styles.row}
      testID="story-rings"
    >
      {rings.map((ring, index) => {
        const name = ring.name ?? t('feed.someone');
        return (
          <Animated.View
            key={ring.senderId}
            entering={
              theme.reducedMotion
                ? undefined
                : FadeInRight.delay(index * motion.stagger).duration(motion.duration.md)
            }
          >
            <Pressable
              testID={`story-ring-${ring.senderId}`}
              accessibilityRole="button"
              accessibilityLabel={
                ring.unseen
                  ? t('feed.ring.a11y.new', { name, count: ring.count })
                  : t('feed.ring.a11y', { name, count: ring.count })
              }
              accessibilityHint={t('feed.ring.hint')}
              onPress={() => onOpen(ring.openId)}
              style={styles.item}
            >
              <Avatar
                id={ring.senderId}
                name={name}
                size={size.avatar.lg + space[2]}
                ring={ring.unseen ? 'unseen' : 'seen'}
              />
              <View accessibilityElementsHidden importantForAccessibility="no-hide-descendants">
                <Text
                  variant="label.sm"
                  tone={ring.unseen ? 'ink' : 'muted'}
                  numberOfLines={1}
                  style={styles.name}
                >
                  {name}
                </Text>
              </View>
            </Pressable>
          </Animated.View>
        );
      })}
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  row: { gap: space[3], paddingVertical: space[1] },
  item: { alignItems: 'center', gap: space[1], width: size.avatar.lg + space[4] },
  name: { textAlign: 'center' },
});
