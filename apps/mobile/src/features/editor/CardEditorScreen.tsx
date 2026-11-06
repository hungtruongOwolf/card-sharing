import { Canvas, createPicture, Picture, Skia, type SkImage } from '@shopify/react-native-skia';
import { useEffect, useMemo, useReducer, useState } from 'react';
import {
  StyleSheet,
  Switch,
  useWindowDimensions,
  View,
  type AccessibilityActionEvent,
} from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import { useSharedValue } from 'react-native-reanimated';

import type { Mask } from '@card-sharing/canvas-engine';
import {
  killedIds,
  MAX_EFFECTS,
  registry as shippedRegistry,
  type EffectEntry,
  type ParamValue,
  type Registry,
  type RemoteRegistryConfig,
} from '@card-sharing/effects-registry';
import { space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import type { TextFilterConfig } from '../../text';
import { Button, Screen, Text, useTheme, useToast } from '../../ui';
import type { CapturedPhoto } from '../capture';
import {
  loadEffectFonts,
  MOVE_STEP,
  movePlacement,
  noAnchorProvider,
  placeEffect,
  placementTargets,
  resolveAnchors,
  type AnchorProvider,
  type NativeAnchors,
  type PlacementTarget,
} from '../effects';
import { CAPTION_MAX_GRAPHEMES, CaptionSheet } from '../captions';
import { Squish } from '../feedback';
import { downscalePhoto, drawCard, previewSize, subjectFor } from './cardScene';
import { EffectTile, unavailableReason } from './EffectTile';
import {
  addEffect,
  canRedo,
  canUndo,
  editorReducer,
  initialHistory,
  replacedBy,
  styleChoice,
  type CardDraft,
  type StyleChoice,
} from './editorState';
import { planFinishModes } from './liveFinish';
import { LiveFinishPreview } from './LiveFinishPreview';
import type { MaskSource } from './maskSource';
import { ParamControls } from './ParamControls';
import {
  defaultCut,
  deviceContext,
  pickerTabs,
  type PickerSlot,
  type PickerTab,
  type PickerTile,
} from './pickerTabs';
import { thumbSize, useTileThumbnails } from './tileThumbnails';

const MAX_PREVIEW_WIDTH = 360;
const MAX_PREVIEW_HEIGHT = 440;
/** Long side of the decoded photo kept for previews (covers a 360 pt preview at 3x). */
const PREVIEW_PHOTO_MAX_SIDE = 1080;

export type LoadImage = (uri: string) => Promise<SkImage | null>;

/** Decodes a photo from app-private storage with Skia. */
export const loadImageFromUri: LoadImage = async (uri) =>
  Skia.Image.MakeImageFromEncoded(await Skia.Data.fromURI(uri));

export interface CardEditorScreenProps {
  photo: CapturedPhoto;
  maskSource: MaskSource;
  appVersion: string;
  /** `GET /v1/config` flags and kill switches; undefined while loading or offline. */
  remoteConfig?: RemoteRegistryConfig;
  /** Caption length limit in graphemes (`/v1/config` `limits.caption_max_chars`); defaults to 80. */
  captionMax?: number;
  /** Served text filter hints (`/v1/config` `text_filter`); without them captions get length checks only. */
  textFilter?: TextFilterConfig;
  loadImage?: LoadImage;
  registry?: Registry;
  /**
   * Re-edit a saved card (drafts tray): the editor starts from these choices instead of the default sticker.
   * Choices the device or config no longer allows fall back to the defaults like any hidden selection.
   */
  initialDraft?: CardDraft;
  /** Called with the shown draft whenever it changes (auto-save to the drafts tray, T-68). */
  onDraftChange?: (draft: CardDraft) => void;
  /** Native face / pet landmarks (T-115..T-117); without one, anchors come from the mask heuristic. */
  anchorSource?: AnchorProvider;
  /** Loads the fonts text effects draw with (tests inject one). */
  loadFonts?: () => Promise<void>;
  /** Back to capture (also the way out of the error state). */
  onBack: () => void;
  /** Next step for the finished draft (send, a later ticket). Without it the editor has no Next button. */
  onNext?: (draft: CardDraft) => void;
}

type Loaded =
  | { status: 'loading' }
  | { status: 'error' }
  | { status: 'ready'; image: SkImage; mask: Mask | null; native: NativeAnchors | null };

/**
 * Card editor (T-64, US-10/US-11): from a captured or imported photo to a styled card. Loads the photo and the
 * subject mask (null = whole-photo card), then hands over to the editor proper.
 */
export function CardEditorScreen(props: CardEditorScreenProps) {
  const {
    photo,
    maskSource,
    loadImage = loadImageFromUri,
    anchorSource = noAnchorProvider,
    loadFonts = loadEffectFonts,
    onBack,
  } = props;
  const [loaded, setLoaded] = useState<Loaded>({ status: 'loading' });

  useEffect(() => {
    let cancelled = false;
    // A failed segmentation is not an error: the card falls back to the whole photo.
    const mask = maskSource.maskFor(photo).catch(() => null);
    // Landmarks and fonts are optional too: effects fall back to the mask heuristic and blank text.
    const native = anchorSource.anchorsFor(photo).catch(() => null);
    void Promise.all([loadImage(photo.uri), mask, native, loadFonts().catch(() => undefined)])
      .then(([image, subjectMask, nativeAnchors]) => {
        if (cancelled) return;
        setLoaded(
          image
            ? {
                status: 'ready',
                image: downscalePhoto(image, PREVIEW_PHOTO_MAX_SIDE),
                mask: subjectMask,
                native: nativeAnchors,
              }
            : { status: 'error' },
        );
      })
      .catch(() => {
        if (!cancelled) setLoaded({ status: 'error' });
      });
    return () => {
      cancelled = true;
    };
  }, [photo, maskSource, loadImage, anchorSource, loadFonts]);

  if (loaded.status === 'loading') {
    return (
      <Screen testID="editor-loading" centered>
        <Text tone="muted">{t('editor.loading')}</Text>
      </Screen>
    );
  }
  if (loaded.status === 'error') {
    return (
      <Screen title={t('editor.error.title')} testID="editor-error">
        <Text tone="muted">{t('editor.error.body')}</Text>
        <Button block label={t('editor.error.back')} onPress={onBack} />
      </Screen>
    );
  }
  return <Editor {...props} image={loaded.image} mask={loaded.mask} native={loaded.native} />;
}

interface EditorProps extends CardEditorScreenProps {
  image: SkImage;
  mask: Mask | null;
  native: NativeAnchors | null;
}

/** The selected entry of a tab, falling back to the default when the stored one is no longer listed. */
function resolveSelection(tab: PickerTab, draft: CardDraft, hasSubject: boolean) {
  const stored = tab.slot === 'cut' ? draft.cut : draft.finish;
  const listed = stored ? tab.entries.find((entry) => entry.id === stored.id) : undefined;
  if (listed && stored) return { entry: listed, choice: stored };
  const fallback = tab.optional ? undefined : defaultCut(tab.entries, hasSubject);
  return fallback ? { entry: fallback, choice: styleChoice(fallback) } : null;
}

function Editor({
  image,
  mask,
  native,
  appVersion,
  remoteConfig,
  registry = shippedRegistry,
  captionMax = CAPTION_MAX_GRAPHEMES,
  textFilter,
  initialDraft,
  onDraftChange,
  onNext,
}: EditorProps) {
  const { width: windowWidth } = useWindowDimensions();
  const size = previewSize(
    { width: image.width(), height: image.height() },
    Math.min(windowWidth - space[4] * 2, MAX_PREVIEW_WIDTH),
    MAX_PREVIEW_HEIGHT,
  );
  const subject = useMemo(() => (mask ? subjectFor(mask, size.width) : null), [mask, size.width]);
  const [history, dispatch] = useReducer(editorReducer, undefined, () => {
    if (initialDraft) return { past: [], present: initialDraft, future: [], gestureBase: null };
    const tabs = pickerTabs(registry, deviceContext(appVersion, subject !== null), remoteConfig);
    const cut = defaultCut(tabs[0]?.entries ?? [], subject !== null);
    return initialHistory(cut ?? registry.byKind('cutStyle')[0]!);
  });
  const [activeSlot, setActiveSlot] = useState<PickerSlot>('cut');
  /** Index in `draft.effects` of the effect whose controls are shown. */
  const [focusedEffect, setFocusedEffect] = useState<number | null>(null);
  /** Which point of the focused effect a drag on the preview moves. */
  const [placeTarget, setPlaceTarget] = useState<PlacementTarget>('body');
  const dragStart = useSharedValue<Readonly<Record<string, ParamValue>> | null>(null);
  const [captionOpen, setCaptionOpen] = useState(false);
  const toast = useToast();
  const { reducedMotion } = useTheme();
  const press = useSharedValue(0);

  const draft = history.present;
  const hasSubject = subject !== null && !draft.wholePhoto;
  const tabs = pickerTabs(registry, deviceContext(appVersion, hasSubject), remoteConfig);
  const cutTab = tabs.find((tab) => tab.slot === 'cut')!;
  const cut = resolveSelection(cutTab, draft, hasSubject);
  const finishTab = tabs.find((tab) => tab.slot === 'finish')!;
  const finish = resolveSelection(finishTab, draft, hasSubject);
  const effectsTab = tabs.find((tab) => tab.slot === 'effects');
  // Effects the device or config no longer allows are dropped from what is shown and sent (like a hidden cut).
  const effects = listedEffects(effectsTab, draft.effects);
  const shown: CardDraft | null = cut
    ? {
        cut: cut.choice,
        finish: finish?.choice ?? null,
        effects: effects.map(({ choice }) => choice),
        wholePhoto: draft.wholePhoto,
        caption: draft.caption,
      }
    : null;

  const shownJson = JSON.stringify(shown);
  useEffect(() => {
    if (shown) onDraftChange?.(shown);
    // `shown` is rebuilt every render; its JSON is the stable identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shownJson]);

  const scene = useMemo(
    () => ({ width: size.width, height: size.height, photo: image, subject }),
    [size.width, size.height, image, subject],
  );
  // The preview is the focused card: a live finish (holo) animates there unless reduced motion is on.
  const [finishMode] = planFinishModes([finish?.entry ?? null], { reducedMotion });
  const liveFinish = finishMode === 'live' && shown?.finish ? shown.finish : null;
  // A live finish is drawn by LiveFinishPreview over a picture of the card without it.
  const drawn: CardDraft | null = shown && liveFinish ? { ...shown, finish: null } : shown;
  const drawnKey = JSON.stringify(drawn);
  const picture = useMemo(
    () =>
      drawn
        ? createPicture(
            (canvas) =>
              drawCard(canvas, registry, drawn, scene, { killed: killedIds(remoteConfig) }),
            size,
          )
        : null,
    // `drawn` is rebuilt every render; its JSON is the stable identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [drawnKey, scene, registry, remoteConfig],
  );

  const activeTab = tabs.find((tab) => tab.slot === activeSlot) ?? cutTab;
  const focused = effects.find(({ index }) => index === focusedEffect) ?? null;
  const active = activeTab.slot === 'cut' ? cut : activeTab.slot === 'finish' ? finish : focused;
  const isSelected = (entry: EffectEntry) =>
    activeTab.slot === 'effects'
      ? effects.some(({ choice }) => choice.id === entry.id)
      : entry.id === active?.entry.id;
  const pickEffect = (entry: EffectEntry, another = false) => {
    const held = effects.find(({ choice }) => choice.id === entry.id);
    if (held && !another) {
      setFocusedEffect(held.index);
      return;
    }
    const next = addEffect(draft.effects, entry, registry);
    if (!next) {
      toast.show({ message: t('editor.effects.full', { max: MAX_EFFECTS }) });
      return;
    }
    const replaced = replacedBy(draft.effects, entry, registry)
      .map((id) => registry.get(id))
      .filter((other) => other !== undefined && other.id !== entry.id);
    if (replaced.length > 0) {
      toast.show({
        message: t('editor.effects.replaced', {
          name: t(entry.name as MessageKey),
          replaced: replaced.map((other) => t(other!.name as MessageKey)).join(', '),
        }),
      });
    }
    // Anchored effects start on the face or head (native landmarks, else the subject heuristic); all stay draggable.
    const anchors = resolveAnchors(native, hasSubject ? mask : null);
    dispatch({ type: 'addEffect', entry, registry, params: placeEffect(entry, anchors) });
    setPlaceTarget('body');
    setFocusedEffect(next.length - 1);
  };
  const select = (entry: EffectEntry | null) =>
    dispatch(
      activeTab.slot === 'cut' && entry
        ? { type: 'select', slot: 'cut', entry }
        : { type: 'select', slot: 'finish', entry },
    );
  const setParams = (entry: EffectEntry, params: Record<string, unknown>, transient?: boolean) =>
    dispatch(
      activeTab.slot === 'effects'
        ? { type: 'setEffectParams', index: focused?.index ?? -1, entry, params, transient }
        : { type: 'setParams', slot: activeTab.slot, entry, params, transient },
    );
  const thumbnails = useTileThumbnails(registry, activeTab, shown, image, mask);
  const tileSize = thumbSize(image.width() / image.height());
  const pressTile = (tile: PickerTile) => {
    if (tile.state.kind === 'available') {
      if (activeTab.slot === 'effects') pickEffect(tile.entry);
      else select(tile.entry);
    } else if (tile.state.kind === 'locked') toast.show({ message: t('editor.tile.locked.toast') });
    else toast.show({ message: unavailableReason(tile.state.reason) });
  };

  const targets = activeTab.slot === 'effects' && focused ? placementTargets(focused.entry) : [];
  const target = targets.includes(placeTarget) ? placeTarget : targets[0];
  const place = (
    dx: number,
    dy: number,
    start: Readonly<Record<string, ParamValue>>,
    transient: boolean,
  ) => {
    if (!focused || !target) return;
    dispatch({
      type: 'setEffectParams',
      index: focused.index,
      entry: focused.entry,
      params: movePlacement(focused.choice.params, target, start, dx, dy),
      transient,
    });
  };
  // Drag on the preview moves the focused effect (or its tail): the manual placement fallback for anchors.
  const placeGesture = Gesture.Pan()
    .withTestId('editor-place')
    .enabled(target !== undefined)
    .runOnJS(true)
    .onBegin(() => {
      dragStart.set(focused?.choice.params ?? null);
    })
    .onUpdate((event) => {
      const start = dragStart.get();
      if (start) {
        place(event.translationX / size.width, event.translationY / size.height, start, true);
      }
    })
    .onFinalize(() => {
      dragStart.set(null);
      dispatch({ type: 'settle' });
    });
  const placeActions = target
    ? (['left', 'right', 'up', 'down'] as const).map((name) => ({
        name: `move-${name}`,
        label: t(`editor.place.${name}`),
      }))
    : undefined;
  const onPlaceAction = (event: AccessibilityActionEvent) => {
    const step: Record<string, [number, number]> = {
      'move-left': [-MOVE_STEP, 0],
      'move-right': [MOVE_STEP, 0],
      'move-up': [0, -MOVE_STEP],
      'move-down': [0, MOVE_STEP],
    };
    const delta = step[event.nativeEvent.actionName];
    if (delta && focused) place(delta[0], delta[1], focused.choice.params, false);
  };

  const baseLabel = cut
    ? finish
      ? t('editor.preview.labelWithFinish', {
          style: t(cut.entry.a11yLabel as MessageKey),
          finish: t(finish.entry.a11yLabel as MessageKey),
        })
      : t('editor.preview.label', { style: t(cut.entry.a11yLabel as MessageKey) })
    : undefined;
  const previewLabel =
    baseLabel && effects.length > 0
      ? t('editor.preview.labelWithEffects', {
          label: baseLabel,
          effects: effects.map(({ entry }) => t(entry.a11yLabel as MessageKey)).join(', '),
        })
      : baseLabel;

  return (
    <Screen title={t('editor.title')} testID="editor">
      <View style={styles.row}>
        <Button
          label={t('editor.undo')}
          variant="ghost"
          disabled={!canUndo(history)}
          onPress={() => dispatch({ type: 'undo' })}
        />
        <Button
          label={t('editor.redo')}
          variant="ghost"
          disabled={!canRedo(history)}
          onPress={() => dispatch({ type: 'redo' })}
        />
      </View>
      <Squish style={styles.center} press={press} gestureTestId="editor-squish">
        <GestureDetector gesture={placeGesture}>
          <View
            style={[styles.preview, size]}
            accessible
            accessibilityRole="image"
            accessibilityLabel={previewLabel}
            accessibilityHint={target ? t('editor.place.hint') : undefined}
            accessibilityActions={placeActions}
            onAccessibilityAction={onPlaceAction}
            testID="editor-preview"
          >
            {picture && liveFinish ? (
              <LiveFinishPreview
                card={{ picture }}
                width={size.width}
                height={size.height}
                intensity={Number(liveFinish.params.intensity ?? 1)}
                press={press}
              />
            ) : picture ? (
              <Canvas style={size}>
                <Picture picture={picture} />
              </Canvas>
            ) : (
              <Text tone="muted">{t('editor.unavailable')}</Text>
            )}
          </View>
        </GestureDetector>
      </Squish>
      {draft.caption ? (
        <Text variant="hand.md" style={styles.caption} testID="editor-caption">
          {draft.caption}
        </Text>
      ) : null}
      <Button
        label={t(draft.caption ? 'editor.caption.edit' : 'editor.caption.add')}
        variant="secondary"
        onPress={() => setCaptionOpen(true)}
      />
      <CaptionSheet
        visible={captionOpen}
        caption={draft.caption}
        max={captionMax}
        textFilter={textFilter}
        onSave={(text) => dispatch({ type: 'setCaption', text })}
        onClose={() => setCaptionOpen(false)}
      />
      {subject ? (
        <View style={styles.row}>
          <Text style={styles.shrink}>{t('editor.wholePhoto')}</Text>
          <Switch
            accessibilityLabel={t('editor.wholePhoto')}
            accessibilityHint={t('editor.wholePhoto.hint')}
            value={draft.wholePhoto}
            onValueChange={(value) => dispatch({ type: 'setWholePhoto', value })}
          />
        </View>
      ) : (
        <Text tone="muted">{t('editor.noSubject')}</Text>
      )}
      <View style={styles.row} accessibilityRole="tablist">
        {tabs.map((tab) => (
          <Button
            key={tab.slot}
            label={t(tab.labelKey)}
            variant={tab.slot === activeTab.slot ? 'primary' : 'secondary'}
            accessibilityRole="tab"
            accessibilityState={{ selected: tab.slot === activeTab.slot }}
            onPress={() => setActiveSlot(tab.slot)}
          />
        ))}
      </View>
      <View style={styles.row}>
        {activeTab.slot === 'finish' ? (
          <Button
            label={t('editor.finish.none')}
            variant={active ? 'secondary' : 'primary'}
            accessibilityState={{ selected: !active }}
            onPress={() => select(null)}
          />
        ) : null}
        {activeTab.tiles.map((tile) => (
          <EffectTile
            key={tile.entry.id}
            tile={tile}
            selected={isSelected(tile.entry)}
            thumbnail={thumbnails[tile.entry.id]}
            size={tileSize}
            onPress={() => pressTile(tile)}
          />
        ))}
      </View>
      {active?.entry.presets?.length ? (
        <View style={styles.row}>
          {active.entry.presets.map((preset) => (
            <Button
              key={preset.id}
              label={t(preset.name as MessageKey)}
              variant="ghost"
              onPress={() => setParams(active.entry, { ...active.choice.params, ...preset.params })}
            />
          ))}
        </View>
      ) : null}
      {activeTab.slot === 'effects' && focused ? (
        <View style={styles.row}>
          <Text style={styles.shrink}>{t(focused.entry.name as MessageKey)}</Text>
          {focused.entry.stackable && draft.effects.length < MAX_EFFECTS ? (
            <Button
              label={t('editor.effects.addAnother')}
              variant="secondary"
              onPress={() => pickEffect(focused.entry, true)}
            />
          ) : null}
          <Button
            label={t('editor.effects.remove', { name: t(focused.entry.name as MessageKey) })}
            variant="ghost"
            onPress={() => {
              dispatch({ type: 'removeEffect', index: focused.index });
              setFocusedEffect(null);
            }}
          />
        </View>
      ) : null}
      {targets.length > 1 ? (
        <View style={styles.row} accessibilityRole="radiogroup">
          {targets.map((option) => (
            <Button
              key={option}
              label={t(option === 'tail' ? 'editor.place.tail' : 'editor.place.body')}
              variant={option === target ? 'primary' : 'secondary'}
              accessibilityRole="radio"
              accessibilityState={{ checked: option === target }}
              onPress={() => setPlaceTarget(option)}
            />
          ))}
        </View>
      ) : null}
      {target ? <Text tone="muted">{t('editor.place.hint')}</Text> : null}
      {active ? (
        <ParamControls
          entry={active.entry}
          params={active.choice.params}
          onChange={(params, transient) => setParams(active.entry, params, transient)}
          onSettle={() => dispatch({ type: 'settle' })}
        />
      ) : null}
      {onNext && shown ? (
        <Button block label={t('editor.next')} onPress={() => onNext(shown)} />
      ) : null}
    </Screen>
  );
}

/** The draft's effects the Effects tab still lists, with their index in `draft.effects`. */
function listedEffects(tab: PickerTab | undefined, choices: readonly StyleChoice[]) {
  return choices.flatMap((choice, index) => {
    const entry = tab?.entries.find((listed) => listed.id === choice.id);
    return entry ? [{ entry, choice, index }] : [];
  });
}

const styles = StyleSheet.create({
  center: { alignSelf: 'center' },
  preview: { alignSelf: 'center', alignItems: 'center', justifyContent: 'center' },
  row: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    justifyContent: 'center',
    alignItems: 'center',
    gap: space[2],
  },
  shrink: { flexShrink: 1 },
  caption: { alignSelf: 'center', textAlign: 'center' },
});
