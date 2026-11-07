import { useInfiniteQuery } from '@tanstack/react-query';
import { StyleSheet, View } from 'react-native';

import { space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, IconButton } from '../../ui';
import { unreadBadge, UNREAD_COUNT_CAP, type ActivityApi } from './activityApi';
import { activityQueryOptions, unreadCountOf } from './activityQuery';
import { Badge } from './Badge';

export interface ActivityButtonProps {
  api: ActivityApi;
  onPress: () => void;
  /** A round bell (home top bar) instead of a labelled button. */
  compact?: boolean;
}

/** Opens the activity tab, with the unread count as a badge ("99+" from 100, where the server stops counting). */
export function ActivityButton({ api, onPress, compact = false }: ActivityButtonProps) {
  const list = useInfiniteQuery(activityQueryOptions(api));
  const count = unreadCountOf(list.data);
  const badge = unreadBadge(count);
  const label =
    badge === null
      ? t('activity.open')
      : count >= UNREAD_COUNT_CAP
        ? t('activity.open.a11y.many')
        : t('activity.open.a11y', { count });
  if (compact) {
    return (
      <View>
        <IconButton icon="bell" variant="surface" accessibilityLabel={label} onPress={onPress} />
        {badge === null ? null : (
          <View style={styles.corner} pointerEvents="none">
            <Badge text={badge} testID="activity-badge" />
          </View>
        )}
      </View>
    );
  }
  return (
    <View style={styles.row}>
      <Button
        label={t('activity.open')}
        accessibilityLabel={label}
        variant="secondary"
        onPress={onPress}
      />
      {badge === null ? null : <Badge text={badge} testID="activity-badge" />}
    </View>
  );
}

const styles = StyleSheet.create({
  row: { flexDirection: 'row', alignItems: 'center', gap: space[2] },
  corner: { position: 'absolute', top: -space[1], end: -space[1] },
});
