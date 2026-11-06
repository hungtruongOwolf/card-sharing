import type { SkImage } from '@shopify/react-native-skia';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback, useMemo, useState } from 'react';
import { Image, Pressable, StyleSheet, useWindowDimensions, View } from 'react-native';
import { useSharedValue } from 'react-native-reanimated';

import { fitCamera, sceneBounds, screenToBoard, type Camera } from '@card-sharing/canvas-engine';
import {
  defaultParams,
  killedIds,
  registry as shippedRegistry,
  type EffectEntry,
  type Registry,
  type RemoteRegistryConfig,
} from '@card-sharing/effects-registry';
import { radius, space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import { Button, Screen, Sheet, Text, useTheme, useToast } from '../../ui';
import type { Person } from '../friends/friendsApi';
import type { CardItem } from '../sharing/cardsApi';
import { BoardCanvas, type PinPose } from './BoardCanvas';
import { BoardMembersSheet, membersQueryKey } from './BoardMembersSheet';
import type { BoardMembersApi } from './boardMembersApi';
import {
  BOARD_SIZE,
  canEditPin,
  clampToBoard,
  pinGeometry,
  type Board,
  type BoardPin,
} from './boardModel';
import {
  addDecorationOp,
  addPinOp,
  applyOps,
  BOARD_PIN_LIMIT,
  bringToFrontOp,
  pinActionOp,
  removePinOp,
  updatePinOp,
  type NewPinContent,
  type PinAction,
  type PinOp,
} from './boardOps';
import { hasBoardRenderer } from './boardDecor';
import type { BoardRepository } from './boardRepository';
import type { PinDisclosure } from './pinDisclosure';
import type { TextureCache } from './textureCache';
import { boardQueryKey, useBoardSync } from './useBoardSync';
import { uuidv7 } from './uuidv7';

export { boardQueryKey };

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
  /** Shared boards (T-108): members, invitations, leave. Absent on local-only boards. */
  members?: BoardMembersApi;
  /** My current friends, the only people I can invite. */
  friends?: () => Promise<readonly Person[]>;
  /** The one-time explanation that only recipients of the card see it on the board (AC-19.5 as resolved by AC-19.7). */
  disclosure?: PinDisclosure;
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
  const sync = useBoardSync(repository, boardId);
  const board = sync.status === 'removed' ? null : query.data;
  return (
    <Screen title={board?.title ?? t('boards.title')} scroll={false} edges={['top']} testID="board">
      <View style={styles.toolbar}>
        <Button label={t('common.close')} variant="secondary" onPress={onClose} />
        {repository.sync && board ? (
          <Button
            label={t('boards.refresh')}
            variant="ghost"
            loading={sync.status === 'syncing'}
            onPress={() => void sync.sync()}
          />
        ) : null}
      </View>
      {query.isPending ? <Text tone="muted">{t('boards.loading')}</Text> : null}
      {sync.status === 'removed' ? (
        <Text tone="danger">{t('boards.sync.removed')}</Text>
      ) : query.isError || board === null ? (
        <Text tone="danger">{t('boards.notFound')}</Text>
      ) : null}
      {board && sync.status === 'offline' ? (
        <Text tone="muted" accessibilityLiveRegion="polite">
          {t('boards.sync.offline', { count: sync.pending })}
        </Text>
      ) : null}
      {board ? (
        <BoardEditor
          key={board.id}
          board={board}
          repository={repository}
          onEdited={sync.schedule}
          onLeft={() => {
            void sync.sync();
            onClose();
          }}
          {...rest}
        />
      ) : null}
    </Screen>
  );
}

interface BoardEditorProps extends Omit<BoardScreenProps, 'boardId' | 'onClose'> {
  board: Board;
  /** My edits were queued: sync soon. */
  onEdited: () => void;
  /** I left this shared board. */
  onLeft: () => void;
}

function BoardEditor({
  board,
  repository,
  textures,
  pinnableCards,
  registry,
  remoteConfig,
  onEdited,
  onLeft,
  members,
  friends,
  disclosure,
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
  const [sheet, setSheet] = useState<'cards' | 'backdrop' | 'decorate' | 'members' | null>(null);
  const [disclosing, setDisclosing] = useState<CardItem | null>(null);
  // A server board: membership comes from the server (a local board has no role and is mine alone).
  const membersApi = board.role ? members : undefined;
  const memberList = useQuery({
    queryKey: membersQueryKey(board.id),
    queryFn: () => membersApi!.members(board.id),
    enabled: membersApi !== undefined,
  });
  const shared = board.role === 'collaborator' || (memberList.data?.length ?? 1) > 1;
  const canEdit = useCallback((pin: BoardPin) => canEditPin(board, pin), [board]);
  const decor = useMemo(
    () => boardDecorChoices(registry ?? shippedRegistry, remoteConfig),
    [registry, remoteConfig],
  );
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
        onEdited();
      });
    },
    [board, queryClient, repository, onEdited],
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
    const restore: NewPinContent | null =
      content.kind === 'card'
        ? { card: content.card }
        : content.kind === 'decoration'
          ? { decoration: content.decoration }
          : null;
    toast.show({
      message: t('boards.removed'),
      action: restore
        ? {
            label: t('common.undo'),
            onPress: () =>
              apply([
                {
                  type: 'pin.add',
                  opId: uuidv7(),
                  pin: { ...pinFields(pin), id: uuidv7(), ...restore },
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
  const place = (makeOp: (centre: { x: number; y: number }) => PinOp) => {
    setSheet(null);
    if (board.pins.length >= BOARD_PIN_LIMIT) {
      toast.show({ message: t('boards.limit', { max: BOARD_PIN_LIMIT }), variant: 'error' });
      return;
    }
    const op = makeOp(screenToBoard(camera.get(), { x: view.width / 2, y: view.height / 2 }));
    apply([op]);
    if (op.type === 'pin.add') setSelectedId(op.pin.id);
  };
  /** My own card on a shared board: explain once that every member can see it (AC-19.5). */
  const pinCard = async (card: CardItem) => {
    if (card.isMine && shared && disclosure && !(await disclosure.seen())) {
      setSheet(null);
      setDisclosing(card);
      return;
    }
    place((centre) => addPinOp(board, card, centre));
  };
  const confirmDisclosure = () => {
    const card = disclosing;
    setDisclosing(null);
    if (!card) return;
    void disclosure?.markSeen().catch(() => undefined);
    place((centre) => addPinOp(board, card, centre));
  };
  const setBackdrop = (entry: EffectEntry) => {
    setSheet(null);
    const backdrop = { id: entry.id, version: entry.version, params: defaultParams(entry) };
    const key = boardQueryKey(board.id);
    queryClient.setQueryData(key, { ...board, backdrop });
    void repository.setBackdrop(board.id, backdrop).then((next) => {
      if (next) queryClient.setQueryData(key, next);
      onEdited();
    });
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
          <>
            <Button label={t('boards.add')} onPress={() => setSheet('cards')} />
            <Button
              label={t('boards.decorate.open')}
              variant="secondary"
              onPress={() => setSheet('decorate')}
            />
            {board.role !== 'collaborator' ? (
              <Button
                label={t('boards.backdrop.open')}
                variant="secondary"
                onPress={() => setSheet('backdrop')}
              />
            ) : null}
            {membersApi ? (
              <Button
                label={t('boards.members.open')}
                variant="secondary"
                onPress={() => setSheet('members')}
              />
            ) : null}
          </>
        )}
      </View>
      <Text tone="muted">
        {board.pins.length === 0 ? t('boards.empty') : t('boards.edit.hint')}
      </Text>
      <View style={styles.canvas}>
        <BoardCanvas
          pins={board.pins}
          backdropId={board.backdrop?.id ?? null}
          width={view.width}
          height={view.height}
          textures={textures}
          initialCamera={initialCamera}
          camera={camera}
          registry={registry}
          remoteConfig={remoteConfig}
          editable
          canEdit={canEdit}
          selectedId={selectedId}
          onSelect={setSelectedId}
          onPickUp={onPickUp}
          onCommit={onCommit}
          onAction={onAction}
        />
      </View>
      {membersApi && friends && board.role ? (
        <BoardMembersSheet
          visible={sheet === 'members'}
          boardId={board.id}
          role={board.role}
          api={membersApi}
          friends={friends}
          onClose={() => setSheet(null)}
          onLeft={onLeft}
        />
      ) : null}
      <Sheet
        visible={disclosing !== null}
        onClose={() => setDisclosing(null)}
        title={t('boards.disclosure.title')}
        testID="pin-disclosure"
      >
        <Text>{t('boards.disclosure.body')}</Text>
        <Button label={t('boards.disclosure.confirm')} onPress={confirmDisclosure} />
        <Button
          label={t('boards.disclosure.cancel')}
          variant="ghost"
          onPress={() => setDisclosing(null)}
        />
      </Sheet>
      <PinPicker
        visible={sheet === 'cards'}
        source={pinnableCards}
        onPick={(card) => void pinCard(card)}
        onClose={() => setSheet(null)}
      />
      <Sheet
        visible={sheet === 'decorate'}
        onClose={() => setSheet(null)}
        title={t('boards.decorate.title')}
        testID="decorate-sheet"
      >
        <View style={styles.grid}>
          {decor.decorations.map((entry) => (
            <Button
              key={entry.id}
              variant="secondary"
              label={t(entry.name as MessageKey)}
              accessibilityLabel={t(entry.a11yLabel as MessageKey)}
              onPress={() => place((centre) => addDecorationOp(board, entry, centre))}
            />
          ))}
        </View>
      </Sheet>
      <Sheet
        visible={sheet === 'backdrop'}
        onClose={() => setSheet(null)}
        title={t('boards.backdrop.title')}
        testID="backdrop-sheet"
      >
        <View style={styles.grid} accessibilityRole="radiogroup">
          {decor.backdrops.map((entry) => {
            const chosen = (board.backdrop?.id ?? DEFAULT_BACKDROP_ID) === entry.id;
            return (
              <Button
                key={entry.id}
                variant={chosen ? 'primary' : 'secondary'}
                label={t(entry.name as MessageKey)}
                accessibilityLabel={t(entry.a11yLabel as MessageKey)}
                accessibilityRole="radio"
                accessibilityState={{ selected: chosen }}
                onPress={() => setBackdrop(entry)}
              />
            );
          })}
        </View>
      </Sheet>
    </View>
  );
}

/** The backdrop a board without one shows (`drawBackdrop` falls back to it). */
const DEFAULT_BACKDROP_ID = 'bg.paper';

/**
 * Backdrops and decorations the board offers: registry entries of those kinds that this app can draw and that
 * `/v1/config` has not switched off (the editor's own picker never lists them).
 */
export function boardDecorChoices(registry: Registry, config: RemoteRegistryConfig | undefined) {
  const killed = killedIds(config);
  const usable = (entry: EffectEntry) => hasBoardRenderer(entry) && !killed.has(entry.id);
  return {
    backdrops: registry.byKind('backdrop').filter(usable),
    decorations: registry.byKind('decoration').filter(usable),
  };
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
