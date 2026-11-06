import type { SkImage } from '@shopify/react-native-skia';
import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import { StyleSheet, useWindowDimensions, View } from 'react-native';

import { fitCamera, sceneBounds } from '@card-sharing/canvas-engine';
import type { Registry, RemoteRegistryConfig } from '@card-sharing/effects-registry';
import { space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Text } from '../../ui';
import { BoardCanvas } from './BoardCanvas';
import { BOARD_SIZE, pinGeometry, type Board } from './boardModel';
import type { BoardRepository } from './boardRepository';
import type { TextureCache } from './textureCache';

export const boardQueryKey = (id: string) => ['boards', id] as const;

/** Space around the pins when the board opens zoomed to fit. */
const FIT_PADDING = space[6];
/** Room for the title and the close button above the canvas. */
const CHROME_HEIGHT = 140;

export interface BoardScreenProps {
  repository: BoardRepository;
  boardId: string;
  textures: TextureCache<SkImage>;
  registry?: Registry;
  remoteConfig?: RemoteRegistryConfig;
  onClose: () => void;
}

/** Initial camera: every pin in view, or the whole board area when it is empty. */
export function openingCamera(board: Board, view: { width: number; height: number }) {
  const area = sceneBounds(board.pins.map(pinGeometry)) ?? {
    x: 0,
    y: 0,
    width: BOARD_SIZE.width,
    height: BOARD_SIZE.height,
  };
  return fitCamera(area, view, FIT_PADDING);
}

/** One board full screen (T-103, US-18): the canvas, or an empty / missing state. */
export function BoardScreen({
  repository,
  boardId,
  textures,
  registry,
  remoteConfig,
  onClose,
}: BoardScreenProps) {
  const window = useWindowDimensions();
  const view = useMemo(
    () => ({ width: window.width, height: Math.max(200, window.height - CHROME_HEIGHT) }),
    [window.width, window.height],
  );
  const query = useQuery({
    queryKey: boardQueryKey(boardId),
    queryFn: () => repository.get(boardId),
  });
  const board = query.data;

  return (
    <Screen title={board?.title ?? t('boards.title')} scroll={false} edges={['top']} testID="board">
      <Button label={t('common.close')} variant="secondary" onPress={onClose} />
      {query.isPending ? <Text tone="muted">{t('boards.loading')}</Text> : null}
      {query.isError || board === null ? <Text tone="danger">{t('boards.notFound')}</Text> : null}
      {board ? (
        <View style={styles.canvas}>
          {board.pins.length === 0 ? <Text tone="muted">{t('boards.empty')}</Text> : null}
          <BoardCanvas
            key={board.id}
            pins={board.pins}
            width={view.width}
            height={view.height}
            textures={textures}
            initialCamera={openingCamera(board, view)}
            registry={registry}
            remoteConfig={remoteConfig}
          />
        </View>
      ) : null}
    </Screen>
  );
}

const styles = StyleSheet.create({
  canvas: { marginHorizontal: -space[4], gap: space[2] },
});
