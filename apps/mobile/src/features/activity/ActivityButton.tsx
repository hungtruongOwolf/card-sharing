import { useInfiniteQuery } from '@tanstack/react-query';
import { StyleSheet, View } from 'react-native';

import { space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button } from '../../ui';
import { unreadBadge, UNREAD_COUNT_CAP, type ActivityApi } from './activityApi';
import { activityQueryOptions, unreadCountOf } from './activityQuery';
import { Badge } from './Badge';

export interface ActivityButtonProps {
  api: ActivityApi;
  onPress: () => void;
}

/** Opens the activity tab, with the unread count as a badge ("99+" from 100, where the server stops counting). */
export function ActivityButton({ api, onPress }: ActivityButtonProps) {
  const list = useInfiniteQuery(activityQueryOptions(api));
  const count = unreadCountOf(list.data);
  const badge = unreadBadge(count);
  return (
    <View style={styles.row}>
      <Button
        label={t('activity.open')}
        accessibilityLabel={
          badge === null
            ? t('activity.open')
            : count >= UNREAD_COUNT_CAP
              ? t('activity.open.a11y.many')
              : t('activity.open.a11y', { count })
        }
        variant="secondary"
        onPress={onPress}
      />
      {badge === null ? null : <Badge text={badge} testID="activity-badge" />}
    </View>
  );
}

const styles = StyleSheet.create({
  row: { flexDirection: 'row', alignItems: 'center', gap: space[2] },
});
