import { Canvas, createPicture, Picture, Skia, type SkImage } from '@shopify/react-native-skia';
import { useEffect, useMemo, useReducer, useState } from 'react';
import { StatusBar } from 'expo-status-bar';
import {
  Image,
  ScrollView,
  StyleSheet,
  Switch,
  useWindowDimensions,
  View,
  type AccessibilityActionEvent,
} from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, {
  FadeIn,
  FadeOut,
  SlideInDown,
  SlideOutDown,
  useSharedValue,
} from 'react-native-reanimated';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

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
import {
  gesture as gestureTokens,
  onMedia,
  palette,
  radius,
  space,
  spring,
} from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import type { TextFilterConfig } from '../../text';
import { Button, IconButton, Screen, Text, useTheme, useToast, type IconName } from '../../ui';
import { AiBadge } from '../ai-effects/AiBadge';
import { AiEffectsSheet } from '../ai-effects/AiEffectsSheet';
import { activeAiLook } from '../ai-effects/aiLook';
import type { AiEffectsController } from '../ai-effects/useAiEffects';
import type { CapturedPhoto } from '../capture';
import { canShareImages, exportDraftPng, shareImage } from '../export';
import {
  loadEffectFonts,
  MOVE_STEP,
  movePlacement,
  NO_GUIDES,
  noAnchorProvider,
  pickSurprise,
  placeEffect,
  placementTargets,
  resizeRotate,
  resolveAnchors,
  snapToCentre,
  surpriseParams,
  transformable,
  type AnchorProvider,
  type NativeAnchors,
  type PlacementTarget,
  type SnapGuides,
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

const MAX_PREVIEW_WIDTH = 420;
/** Share of the window height the canvas may use (the rest: top bar, tray, Next). */
const PREVIEW_HEIGHT_SHARE = 0.5;
/** The editor is always night, like a camera: the card is the only colour on screen. */
const STAGE = palette.night[950];

/**
 * The tool rail (sprint 9). Shape, finish and effects are the registry slots; Stickers and Text are views of the
 * effects slot filtered by registry tag, so adding a sticker or a text style is a registry entry, never an editor
 * change.
 */
interface Tool {
  key: 'cut' | 'finish' | 'stickers' | 'text' | 'effects';
  slot: PickerSlot;
  icon: IconName;
  labelKey: MessageKey;
  /** Only entries with this tag (effects slot views). */
  tag?: string;
}
const TOOLS: readonly Tool[] = [
  { key: 'cut', slot: 'cut', icon: 'cut', labelKey: 'editor.tab.cut' },
  { key: 'finish', slot: 'finish', icon: 'finish', labelKey: 'editor.tab.finish' },
  {
    key: 'stickers',
    slot: 'effects',
    icon: 'sticker',
    labelKey: 'editor.tab.stickers',
    tag: 'sticker',
  },
  { key: 'text', slot: 'effects', icon: 'text', labelKey: 'editor.tab.text', tag: 'text' },
  { key: 'effects', slot: 'effects', icon: 'effects', labelKey: 'editor.tab.effects' },
];
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
  /** Opens the share sheet with an exported PNG (T-127); resolves false when the platform cannot. */
  shareExport?: (png: Uint8Array) => Promise<boolean>;
  /** Back to capture (also the way out of the error state). */
  onBack: () => void;
  /** Next step for the finished draft (send, a later ticket). Without it the editor has no Next button. */
  onNext?: (draft: CardDraft) => void;
  /**
   * Tier 2 cloud AI effects. The AI entry shows only when the server says they are available for this account
   * (region, age, provider); without a controller there is no AI entry.
   */
  aiEffects?: AiEffectsController;
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
  photo,
  maskSource,
  loadImage = loadImageFromUri,
  loadFonts = loadEffectFonts,
  shareExport = shareImage,
  appVersion,
  remoteConfig,
  registry = shippedRegistry,
  captionMax = CAPTION_MAX_GRAPHEMES,
  textFilter,
  initialDraft,
  onDraftChange,
  onNext,
  aiEffects,
}: EditorProps) {
  const { width: windowWidth, height: windowHeight } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  const size = previewSize(
    { width: image.width(), height: image.height() },
    Math.min(windowWidth - space[16] * 2, MAX_PREVIEW_WIDTH),
    Math.max(240, windowHeight * PREVIEW_HEIGHT_SHARE),
  );
  const subject = useMemo(() => (mask ? subjectFor(mask, size.width) : null), [mask, size.width]);
  const [history, dispatch] = useReducer(editorReducer, undefined, () => {
    if (initialDraft) return { past: [], present: initialDraft, future: [], gestureBase: null };
    const tabs = pickerTabs(registry, deviceContext(appVersion, subject !== null), remoteConfig);
    const cut = defaultCut(tabs[0]?.entries ?? [], subject !== null);
    return initialHistory(cut ?? registry.byKind('cutStyle')[0]!);
  });
  const [activeTool, setActiveTool] = useState<Tool['key']>('cut');
  const [trayOpen, setTrayOpen] = useState(true);
  const [guides, setGuides] = useState<SnapGuides>(NO_GUIDES);
  /** Index in `draft.effects` of the effect whose controls are shown. */
  const [focusedEffect, setFocusedEffect] = useState<number | null>(null);
  /** Which point of the focused effect a drag on the preview moves. */
  const [placeTarget, setPlaceTarget] = useState<PlacementTarget>('body');
  const dragStart = useSharedValue<Readonly<Record<string, ParamValue>> | null>(null);
  const [captionOpen, setCaptionOpen] = useState(false);
  const [aiOpen, setAiOpen] = useState(false);
  /** Signed preview URLs of AI results by job id (short-lived; never stored with the draft). */
  const [aiPreviews, setAiPreviews] = useState<Readonly<Record<string, string>>>({});
  const toast = useToast();
  const theme = useTheme();
  const { reducedMotion } = theme;
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
  const look: CardDraft | null = cut
    ? {
        cut: cut.choice,
        finish: finish?.choice ?? null,
        effects: effects.map(({ choice }) => choice),
        wholePhoto: draft.wholePhoto,
        caption: draft.caption,
      }
    : null;
  // An AI result counts only while it matches what is shown; an edit sets it aside (undo brings it back).
  const aiLook = look && draft.ai ? activeAiLook({ ...look, ai: draft.ai }) : null;
  const shown: CardDraft | null = look && aiLook ? { ...look, ai: aiLook } : look;
  const aiPreview = aiLook ? (aiPreviews[aiLook.jobId] ?? null) : null;
  const aiAvailable = aiEffects?.overview?.available === true;

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

  // Tools whose slot this device lists, and whose tag filter leaves at least one tile.
  const tools = TOOLS.filter((tool) => {
    const tab = tabs.find((candidate) => candidate.slot === tool.slot);
    return tab && (!tool.tag || tab.tiles.some((tile) => tile.entry.tags.includes(tool.tag!)));
  });
  const tool = tools.find((candidate) => candidate.key === activeTool) ?? tools[0]!;
  const activeTab = tabs.find((tab) => tab.slot === tool.slot) ?? cutTab;
  const shownTiles = tool.tag
    ? activeTab.tiles.filter((tile) => tile.entry.tags.includes(tool.tag!))
    : activeTab.tiles;
  const pickTool = (key: Tool['key']) => {
    if (key === tool.key) {
      setTrayOpen((open) => !open);
      return;
    }
    setActiveTool(key);
    setTrayOpen(true);
  };
  const focused = effects.find(({ index }) => index === focusedEffect) ?? null;
  const active = activeTab.slot === 'cut' ? cut : activeTab.slot === 'finish' ? finish : focused;
  const isSelected = (entry: EffectEntry) =>
    activeTab.slot === 'effects'
      ? effects.some(({ choice }) => choice.id === entry.id)
      : entry.id === active?.entry.id;
  const pickEffect = (
    entry: EffectEntry,
    another = false,
    extra: Readonly<Record<string, ParamValue>> = {},
  ) => {
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
    dispatch({
      type: 'addEffect',
      entry,
      registry,
      params: { ...placeEffect(entry, anchors), ...extra },
    });
    setPlaceTarget('body');
    setFocusedEffect(next.length - 1);
  };
  /** One random available effect with random choices, only on this explicit tap (T-125, AC-21.4). */
  const surprise = () => {
    const entry = pickSurprise(
      effectsTab?.entries ?? [],
      new Set(effects.map(({ choice }) => choice.id)),
    );
    if (!entry) return;
    pickEffect(entry, true, surpriseParams(entry));
    toast.show({ message: t('editor.effects.surprised', { name: t(entry.name as MessageKey) }) });
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
    let params = movePlacement(focused.choice.params, target, start, dx, dy);
    // The body snaps to the centre lines while dragging, with guides (gesture.snapDistance).
    if (target === 'body' && transient) {
      const snapped = snapToCentre(params, size, gestureTokens.snapDistance);
      params = snapped.params;
      setGuides(snapped.guides);
    }
    dispatch({
      type: 'setEffectParams',
      index: focused.index,
      entry: focused.entry,
      params,
      transient,
    });
  };
  const canTransform = focused ? transformable(focused.entry) : { scale: false, rotate: false };
  const transform = (change: { scale?: number; rotationDeg?: number }) => {
    const start = dragStart.get();
    if (!focused || !start) return;
    dispatch({
      type: 'setEffectParams',
      index: focused.index,
      entry: focused.entry,
      params: resizeRotate(
        focused.entry,
        focused.choice.params,
        start,
        change,
        gestureTokens.snapAngle,
      ),
      transient: true,
    });
  };
  const beginGesture = () => {
    if (!dragStart.get()) dragStart.set(focused?.choice.params ?? null);
  };
  const endGesture = () => {
    dragStart.set(null);
    setGuides(NO_GUIDES);
    dispatch({ type: 'settle' });
  };
  // Drag on the preview moves the focused effect (or its tail): the manual placement fallback for anchors.
  const placeGesture = Gesture.Pan()
    .withTestId('editor-place')
    .enabled(target !== undefined)
    .runOnJS(true)
    .onBegin(beginGesture)
    .onUpdate((event) => {
      const start = dragStart.get();
      if (start) {
        place(event.translationX / size.width, event.translationY / size.height, start, true);
      }
    })
    .onFinalize(endGesture);
  // Pinch to resize and twist to rotate the focused layer, together with the drag.
  const pinchGesture = Gesture.Pinch()
    .withTestId('editor-pinch')
    .enabled(canTransform.scale)
    .runOnJS(true)
    .onBegin(beginGesture)
    .onUpdate((event) => transform({ scale: event.scale }))
    .onFinalize(endGesture);
  const rotateGesture = Gesture.Rotation()
    .withTestId('editor-rotate')
    .enabled(canTransform.rotate)
    .runOnJS(true)
    .onBegin(beginGesture)
    .onUpdate((event) => transform({ rotationDeg: (event.rotation * 180) / Math.PI }))
    .onFinalize(endGesture);
  const canvasGesture = Gesture.Simultaneous(placeGesture, pinchGesture, rotateGesture);
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
  const shownLabel = aiLook ? t('editor.ai.preview.label', { name: aiLook.title }) : previewLabel;

  const exportDraft = async () => {
    if (!shown) return;
    // The user's own draft, from the full-resolution photo at the export size (T-127).
    const [full, fullMask] = await Promise.all([
      loadImage(photo.uri).catch(() => null),
      shown.wholePhoto ? null : maskSource.maskFor(photo).catch(() => null),
      loadFonts().catch(() => undefined),
    ]);
    const png = full
      ? exportDraftPng({ registry, draft: shown, photo: full, mask: fullMask })
      : null;
    if (!png || !(await shareExport(png).catch(() => false))) {
      toast.show({ message: t('card.export.failed') });
    }
  };

  return (
    <View testID="editor" style={[styles.root, { backgroundColor: STAGE }]}>
      <StatusBar style="light" />
      <View style={[styles.topBar, { paddingTop: Math.max(insets.top, space[2]) }]}>
        <Text variant="title.md" tone="onMedia" accessibilityRole="header" style={styles.title}>
          {t('editor.title')}
        </Text>
        <IconButton
          icon="undo"
          accessibilityLabel={t('editor.undo')}
          disabled={!canUndo(history)}
          onPress={() => dispatch({ type: 'undo' })}
        />
        <IconButton
          icon="redo"
          accessibilityLabel={t('editor.redo')}
          disabled={!canRedo(history)}
          onPress={() => dispatch({ type: 'redo' })}
        />
        {shown && canShareImages() ? (
          <IconButton
            icon="download"
            accessibilityLabel={t('card.export')}
            accessibilityHint={t('card.export.hint')}
            onPress={exportDraft}
          />
        ) : null}
      </View>
      <View style={styles.stage}>
        <Squish style={styles.center} press={press} gestureTestId="editor-squish">
          <GestureDetector gesture={canvasGesture}>
            <View
              style={[styles.preview, size, { boxShadow: theme.shadow(3) }]}
              accessible
              accessibilityRole="image"
              accessibilityLabel={shownLabel}
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
                <Text tone="onMediaMuted">{t('editor.unavailable')}</Text>
              )}
              {aiPreview ? (
                <Image
                  testID="editor-ai-preview"
                  source={{ uri: aiPreview }}
                  style={[StyleSheet.absoluteFill, styles.aiPreview]}
                  resizeMode="cover"
                  accessibilityIgnoresInvertColors
                />
              ) : null}
              {aiLook ? <AiBadge testID="editor-ai-badge" style={styles.aiBadge} /> : null}
              {guides.vertical ? (
                <View pointerEvents="none" testID="editor-guide-vertical" style={styles.guideV} />
              ) : null}
              {guides.horizontal ? (
                <View pointerEvents="none" testID="editor-guide-horizontal" style={styles.guideH} />
              ) : null}
            </View>
          </GestureDetector>
        </Squish>
        {draft.caption ? (
          <Text variant="hand.md" tone="onMedia" style={styles.caption} testID="editor-caption">
            {draft.caption}
          </Text>
        ) : null}
        {aiLook ? (
          <View style={styles.aiRow} testID="editor-ai-look">
            <Text variant="label.md" tone="onMedia" style={styles.shrink}>
              {t('editor.ai.applied', { name: aiLook.title })}
            </Text>
            <Button
              variant="ghost"
              label={t('editor.ai.remove')}
              onPress={() => dispatch({ type: 'setAiLook', look: null })}
            />
          </View>
        ) : null}
        {aiLook && !aiPreview ? (
          <Text variant="label.sm" tone="onMediaMuted" style={styles.caption}>
            {t('editor.ai.pending')}
          </Text>
        ) : null}
        <View style={styles.rail} accessibilityRole="tablist">
          {tools.map((candidate) => (
            <IconButton
              key={candidate.key}
              icon={candidate.icon}
              accessibilityRole="tab"
              accessibilityLabel={t(candidate.labelKey)}
              selected={candidate.key === tool.key && trayOpen}
              onPress={() => pickTool(candidate.key)}
            />
          ))}
          {aiAvailable ? (
            <IconButton
              testID="editor-ai"
              icon="sparkle"
              accessibilityLabel={t('editor.tab.ai')}
              selected={aiOpen}
              onPress={() => setAiOpen(true)}
            />
          ) : null}
        </View>
      </View>
      {trayOpen ? (
        <Animated.View
          testID="editor-tray"
          entering={
            theme.reducedMotion
              ? FadeIn
              : SlideInDown.springify()
                  .damping(spring.sheet.damping)
                  .stiffness(spring.sheet.stiffness)
                  .mass(spring.sheet.mass)
          }
          exiting={theme.reducedMotion ? FadeOut : SlideOutDown}
          style={[
            styles.tray,
            { backgroundColor: theme.color.bg.paper, boxShadow: theme.shadow(4) },
          ]}
        >
          <View style={[styles.grabber, { backgroundColor: theme.hairline }]} />
          <ScrollView
            style={styles.trayScroll}
            contentContainerStyle={styles.trayContent}
            keyboardShouldPersistTaps="handled"
          >
            <Text variant="title.md" accessibilityRole="header">
              {t(tool.labelKey)}
            </Text>
            {tool.slot === 'cut' ? (
              subject ? (
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
              )
            ) : null}
            <ScrollView
              horizontal
              showsHorizontalScrollIndicator={false}
              contentContainerStyle={styles.tiles}
            >
              {tool.key === 'effects' ? (
                <Button
                  label={t('editor.effects.surprise')}
                  variant="secondary"
                  accessibilityHint={t('editor.effects.surprise.hint')}
                  onPress={surprise}
                />
              ) : null}
              {tool.slot === 'finish' ? (
                <Button
                  label={t('editor.finish.none')}
                  variant={active ? 'secondary' : 'primary'}
                  accessibilityState={{ selected: !active }}
                  onPress={() => select(null)}
                />
              ) : null}
              {shownTiles.map((tile) => (
                <EffectTile
                  key={tile.entry.id}
                  tile={tile}
                  selected={isSelected(tile.entry)}
                  thumbnail={thumbnails[tile.entry.id]}
                  size={tileSize}
                  onPress={() => pressTile(tile)}
                />
              ))}
            </ScrollView>
            {active?.entry.presets?.length ? (
              <View style={styles.row}>
                {active.entry.presets.map((preset) => (
                  <Button
                    key={preset.id}
                    label={t(preset.name as MessageKey)}
                    variant="ghost"
                    onPress={() =>
                      setParams(active.entry, { ...active.choice.params, ...preset.params })
                    }
                  />
                ))}
              </View>
            ) : null}
            {activeTab.slot === 'effects' && focused ? (
              <View style={styles.row}>
                <Text variant="label.lg" style={styles.shrink}>
                  {t(focused.entry.name as MessageKey)}
                </Text>
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
            {target ? (
              <Text tone="muted">
                {canTransform.scale || canTransform.rotate
                  ? t('editor.place.hintTransform')
                  : t('editor.place.hint')}
              </Text>
            ) : null}
            {active ? (
              <ParamControls
                entry={active.entry}
                params={active.choice.params}
                onChange={(params, transient) => setParams(active.entry, params, transient)}
                onSettle={() => dispatch({ type: 'settle' })}
              />
            ) : null}
          </ScrollView>
        </Animated.View>
      ) : null}
      <View
        style={[
          styles.bottomBar,
          {
            backgroundColor: theme.color.bg.paper,
            paddingBottom: Math.max(insets.bottom, space[3]),
          },
        ]}
      >
        <Button
          label={t(draft.caption ? 'editor.caption.edit' : 'editor.caption.add')}
          variant="secondary"
          onPress={() => setCaptionOpen(true)}
        />
        {onNext && shown ? (
          <View style={styles.grow}>
            <Button block label={t('editor.next')} onPress={() => onNext(shown)} />
          </View>
        ) : null}
      </View>
      <CaptionSheet
        visible={captionOpen}
        caption={draft.caption}
        max={captionMax}
        textFilter={textFilter}
        onSave={(text) => dispatch({ type: 'setCaption', text })}
        onClose={() => setCaptionOpen(false)}
      />
      {aiEffects && look ? (
        <AiEffectsSheet
          visible={aiOpen}
          onClose={() => setAiOpen(false)}
          controller={aiEffects}
          draft={look}
          onApply={(applied, previewUrl) => {
            if (previewUrl) setAiPreviews((all) => ({ ...all, [applied.jobId]: previewUrl }));
            dispatch({ type: 'setAiLook', look: applied });
          }}
        />
      ) : null}
    </View>
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
  root: { flex: 1 },
  topBar: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: space[2],
    paddingHorizontal: space[3],
    paddingBottom: space[2],
  },
  title: { flex: 1 },
  stage: { flex: 1, alignItems: 'center', justifyContent: 'center' },
  center: { alignSelf: 'center' },
  preview: {
    alignSelf: 'center',
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: radius.md,
  },
  guideV: {
    position: 'absolute',
    top: 0,
    bottom: 0,
    start: '50%',
    width: 1,
    backgroundColor: onMedia.ink,
  },
  guideH: {
    position: 'absolute',
    start: 0,
    end: 0,
    top: '50%',
    height: 1,
    backgroundColor: onMedia.ink,
  },
  caption: { alignSelf: 'center', textAlign: 'center', marginTop: space[2] },
  aiPreview: { borderRadius: radius.md },
  aiBadge: { position: 'absolute', top: space[2], start: space[2] },
  aiRow: {
    flexDirection: 'row',
    alignItems: 'center',
    alignSelf: 'center',
    gap: space[2],
    marginTop: space[1],
  },
  rail: {
    position: 'absolute',
    end: space[3],
    top: 0,
    bottom: 0,
    justifyContent: 'center',
    gap: space[3],
  },
  tray: {
    borderTopStartRadius: radius.xl,
    borderTopEndRadius: radius.xl,
    paddingTop: space[2],
    maxHeight: '42%',
  },
  grabber: { alignSelf: 'center', width: 36, height: 4, borderRadius: radius.full },
  trayScroll: { flexGrow: 0 },
  trayContent: { gap: space[3], paddingHorizontal: space[4], paddingVertical: space[2] },
  tiles: { gap: space[2], alignItems: 'flex-start', paddingVertical: space[1] },
  row: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: space[2],
  },
  shrink: { flexShrink: 1 },
  bottomBar: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: space[3],
    paddingHorizontal: space[4],
    paddingTop: space[3],
  },
  grow: { flex: 1 },
});
