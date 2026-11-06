import type { SkImage } from '@shopify/react-native-skia';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback, useMemo, useState } from 'react';
import { Image, Pressable, StyleSheet, useWindowDimensions, View } from 'react-native';
import { useSharedValue } from 'react-native-reanimated';

import { fitCamera, sceneBounds, screenToBoard, type Camera } from '@card-sharing/canvas-engine';
import type { Registry, RemoteRegistryConfig } from '@card-sharing/effects-registry';
import { radius, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Sheet, Text, useTheme, useToast } from '../../ui';
import type { CardItem } from '../sharing/cardsApi';
import { BoardCanvas, type PinPose } from './BoardCanvas';
import { BOARD_SIZE, clampToBoard, pinGeometry, type Board, type BoardPin } from './boardModel';
import {
  addPinOp,
  applyOps,
  BOARD_PIN_LIMIT,
  bringToFrontOp,
  pinActionOp,
  removePinOp,
  updatePinOp,
  type PinAction,
  type PinOp,
} from './boardOps';
import type { BoardRepository } from './boardRepository';
import type { TextureCache } from './textureCache';
import { uuidv7 } from './uuidv7';

export const boardQueryKey = (id: string) => ['boards', id] as const;
const PINNABLE_QUERY_KEY = ['boards', 'pinnable'] as const;

/** Space around the pins when the board opens zoomed to fit. */
const FIT_PADDING = space[6];
/** Room for the title and the toolbar above the canvas. */
const CHROME_HEIGHT = 200;
const THUMB = 56;

/** Cards I may pin: my own and those I received (the server re-checks on every pin, ADR-0007). */
export type PinnableCards = () => Promise<readonly CardItem[]>;

export interface BoardScreenProps {
  repository: BoardRepository;
  boardId: string;
  textures: TextureCache<SkImage>;
  pinnableCards: PinnableCards;
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

/** One board full screen (T-103/T-104, US-18): the editable canvas, or a missing state. */
export function BoardScreen({ repository, boardId, onClose, ...rest }: BoardScreenProps) {
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
        <BoardEditor key={board.id} board={board} repository={repository} {...rest} />
      ) : null}
    </Screen>
  );
}

interface BoardEditorProps extends Omit<BoardScreenProps, 'boardId' | 'onClose'> {
  board: Board;
}

function BoardEditor({
  board,
  repository,
  textures,
  pinnableCards,
  registry,
  remoteConfig,
}: BoardEditorProps) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const window = useWindowDimensions();
  const view = useMemo(
    () => ({ width: window.width, height: Math.max(200, window.height - CHROME_HEIGHT) }),
    [window.width, window.height],
  );
  const [initialCamera] = useState(() => openingCamera(board, view));
  const camera = useSharedValue<Camera>(initialCamera);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const selected = board.pins.find((p) => p.id === selectedId) ?? null;

  /** Applies ops optimistically on screen, then through the repository (the sync seam). */
  const apply = useCallback(
    (ops: readonly (PinOp | null)[]) => {
      const real = ops.filter((op): op is PinOp => op !== null);
      if (real.length === 0) return;
      const key = boardQueryKey(board.id);
      const current = queryClient.getQueryData<Board | null>(key) ?? board;
      queryClient.setQueryData(key, applyOps(current, real));
      void repository.apply(board.id, real).then((next) => {
        if (next) queryClient.setQueryData(key, next);
      });
    },
    [board, queryClient, repository],
  );

  const pinById = (id: string): BoardPin | undefined => board.pins.find((p) => p.id === id);

  const onPickUp = (id: string) => {
    setSelectedId(id);
    const pin = pinById(id);
    if (pin) apply([bringToFrontOp(board, pin)]);
  };
  const onCommit = (id: string, pose: PinPose) => {
    const pin = pinById(id);
    if (pin) apply([updatePinOp(pin, clampToBoard(pose))]);
  };
  const remove = (pin: BoardPin) => {
    setSelectedId(null);
    apply([removePinOp(pin)]);
    const content = pin.content;
    toast.show({
      message: t('boards.removed'),
      action:
        content.kind === 'card'
          ? {
              label: t('common.undo'),
              onPress: () =>
                apply([
                  {
                    type: 'pin.add',
                    opId: uuidv7(),
                    pin: { ...pinFields(pin), id: uuidv7(), card: content.card },
                  },
                ]),
            }
          : undefined,
    });
  };
  const onAction = (id: string, action: PinAction) => {
    const pin = pinById(id);
    if (!pin) return;
    if (action === 'remove') remove(pin);
    else apply([pinActionOp(board, pin, action)]);
  };
  const pin = (card: CardItem) => {
    setPickerOpen(false);
    if (board.pins.length >= BOARD_PIN_LIMIT) {
      toast.show({ message: t('boards.limit', { max: BOARD_PIN_LIMIT }), variant: 'error' });
      return;
    }
    const centre = screenToBoard(camera.get(), { x: view.width / 2, y: view.height / 2 });
    const op = addPinOp(board, card, centre);
    apply([op]);
    if (op.type === 'pin.add') setSelectedId(op.pin.id);
  };

  return (
    <View style={styles.editor}>
      <View style={styles.toolbar}>
        {selected ? (
          <>
            <Button
              label={t('boards.action.bringToFront')}
              variant="secondary"
              onPress={() => apply([bringToFrontOp(board, selected)])}
            />
            <Button
              label={t('boards.action.remove')}
              variant="secondary"
              onPress={() => remove(selected)}
            />
            <Button
              label={t('common.dismiss')}
              variant="ghost"
              onPress={() => setSelectedId(null)}
            />
          </>
        ) : (
          <Button label={t('boards.add')} onPress={() => setPickerOpen(true)} />
        )}
      </View>
      <Text tone="muted">
        {board.pins.length === 0 ? t('boards.empty') : t('boards.edit.hint')}
      </Text>
      <View style={styles.canvas}>
        <BoardCanvas
          pins={board.pins}
          width={view.width}
          height={view.height}
          textures={textures}
          initialCamera={initialCamera}
          camera={camera}
          registry={registry}
          remoteConfig={remoteConfig}
          editable
          selectedId={selectedId}
          onSelect={setSelectedId}
          onPickUp={onPickUp}
          onCommit={onCommit}
          onAction={onAction}
        />
      </View>
      <PinPicker
        visible={pickerOpen}
        source={pinnableCards}
        onPick={pin}
        onClose={() => setPickerOpen(false)}
      />
    </View>
  );
}

const pinFields = ({ x, y, scale, rotation, zKey }: BoardPin) => ({ x, y, scale, rotation, zKey });

function cardLabel(card: CardItem): string {
  if (card.isMine) return t('boards.picker.card.mine');
  const name = card.sender.displayName;
  return name ? t('boards.picker.card', { name }) : t('boards.picker.card.someone');
}

interface PinPickerProps {
  visible: boolean;
  source: PinnableCards;
  onPick: (card: CardItem) => void;
  onClose: () => void;
}

/** My cards and received cards to pin (T-104); only cards with a delivered render are offered. */
function PinPicker({ visible, source, onPick, onClose }: PinPickerProps) {
  const theme = useTheme();
  const query = useQuery({ queryKey: PINNABLE_QUERY_KEY, queryFn: source, enabled: visible });
  const cards = (query.data ?? []).filter((card) => card.media !== null);
  return (
    <Sheet visible={visible} onClose={onClose} title={t('boards.picker.title')} testID="pin-picker">
      {query.isPending ? <Text tone="muted">{t('boards.picker.loading')}</Text> : null}
      {query.isError ? <Text tone="danger">{t('boards.picker.error')}</Text> : null}
      {query.isSuccess && cards.length === 0 ? (
        <Text tone="muted">{t('boards.picker.empty')}</Text>
      ) : null}
      <View style={styles.grid}>
        {cards.map((card) => (
          <Pressable
            key={card.id}
            accessibilityRole="button"
            accessibilityLabel={cardLabel(card)}
            onPress={() => onPick(card)}
            style={[
              styles.thumb,
              { minWidth: theme.touchMin, backgroundColor: theme.color.bg.paper },
            ]}
          >
            <Image
              source={{ uri: card.media!.renderUrl }}
              style={styles.thumbImage}
              resizeMode="contain"
              accessibilityIgnoresInvertColors
            />
          </Pressable>
        ))}
      </View>
    </Sheet>
  );
}

const styles = StyleSheet.create({
  editor: { gap: space[2] },
  toolbar: { flexDirection: 'row', flexWrap: 'wrap', gap: space[2] },
  canvas: { marginHorizontal: -space[4] },
  grid: { flexDirection: 'row', flexWrap: 'wrap', gap: space[2] },
  thumb: { padding: space[1], borderRadius: radius.md },
  thumbImage: { width: THUMB, height: THUMB },
});
