import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { StyleSheet, View } from 'react-native';

import { space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Text, useToast } from '../../ui';
import type { BoardRepository } from './boardRepository';

export const BOARDS_QUERY_KEY = ['boards'] as const;

export interface BoardsScreenProps {
  repository: BoardRepository;
  onOpen: (id: string) => void;
  onOpenLab?: () => void;
}

/** My boards (US-18): open one or start a new personal board. */
export function BoardsScreen({ repository, onOpen, onOpenLab }: BoardsScreenProps) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const query = useQuery({ queryKey: BOARDS_QUERY_KEY, queryFn: () => repository.list() });
  const create = useMutation({
    mutationFn: () => repository.create(t('boards.defaultTitle')),
    onSuccess: async (board) => {
      await queryClient.invalidateQueries({ queryKey: BOARDS_QUERY_KEY });
      onOpen(board.id);
    },
    onError: () => toast.show({ message: t('boards.error.create'), variant: 'error' }),
  });

  return (
    <Screen title={t('boards.title')} testID="boards">
      <Button label={t('boards.new')} loading={create.isPending} onPress={() => create.mutate()} />
      {query.isPending ? <Text tone="muted">{t('boards.loading')}</Text> : null}
      {query.data?.length === 0 ? <Text tone="muted">{t('boards.list.empty')}</Text> : null}
      <View style={styles.list}>
        {query.data?.map((board) => (
          <Button
            key={board.id}
            variant="secondary"
            label={t('boards.list.item', { title: board.title, count: board.pinCount })}
            onPress={() => onOpen(board.id)}
          />
        ))}
      </View>
      {onOpenLab ? (
        <Button label={t('boards.lab.open')} variant="ghost" onPress={onOpenLab} />
      ) : null}
    </Screen>
  );
}

const styles = StyleSheet.create({
  list: { gap: space[2] },
});
