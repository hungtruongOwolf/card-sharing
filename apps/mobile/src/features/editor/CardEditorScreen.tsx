import { Canvas, createPicture, Picture, Skia, type SkImage } from '@shopify/react-native-skia';
import { useEffect, useMemo, useReducer, useState } from 'react';
import { StyleSheet, Switch, useWindowDimensions, View } from 'react-native';

import type { Mask } from '@card-sharing/canvas-engine';
import {
  registry as shippedRegistry,
  type EffectEntry,
  type Registry,
  type RemoteRegistryConfig,
} from '@card-sharing/effects-registry';
import { space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import { Button, Screen, Text, useToast } from '../../ui';
import type { CapturedPhoto } from '../capture';
import { CAPTION_MAX_GRAPHEMES, CaptionSheet } from '../captions';
import { downscalePhoto, drawCard, previewSize, subjectFor } from './cardScene';
import { EffectTile, unavailableReason } from './EffectTile';
import {
  canRedo,
  canUndo,
  editorReducer,
  initialHistory,
  styleChoice,
  type CardDraft,
  type StyleSlot,
} from './editorState';
import type { MaskSource } from './maskSource';
import { ParamControls } from './ParamControls';
import {
  defaultCut,
  deviceContext,
  pickerTabs,
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
  loadImage?: LoadImage;
  registry?: Registry;
  /** Back to capture (also the way out of the error state). */
  onBack: () => void;
  /** Next step for the finished draft (send, a later ticket). Without it the editor has no Next button. */
  onNext?: (draft: CardDraft) => void;
}

type Loaded =
  | { status: 'loading' }
  | { status: 'error' }
  | { status: 'ready'; image: SkImage; mask: Mask | null };

/**
 * Card editor (T-64, US-10/US-11): from a captured or imported photo to a styled card. Loads the photo and the
 * subject mask (null = whole-photo card), then hands over to the editor proper.
 */
export function CardEditorScreen(props: CardEditorScreenProps) {
  const { photo, maskSource, loadImage = loadImageFromUri, onBack } = props;
  const [loaded, setLoaded] = useState<Loaded>({ status: 'loading' });

  useEffect(() => {
    let cancelled = false;
    // A failed segmentation is not an error: the card falls back to the whole photo.
    const mask = maskSource.maskFor(photo).catch(() => null);
    void Promise.all([loadImage(photo.uri), mask])
      .then(([image, subjectMask]) => {
        if (cancelled) return;
        setLoaded(
          image
            ? {
                status: 'ready',
                image: downscalePhoto(image, PREVIEW_PHOTO_MAX_SIDE),
                mask: subjectMask,
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
  }, [photo, maskSource, loadImage]);

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
  return <Editor {...props} image={loaded.image} mask={loaded.mask} />;
}

interface EditorProps extends CardEditorScreenProps {
  image: SkImage;
  mask: Mask | null;
}

/** The selected entry of a tab, falling back to the default when the stored one is no longer listed. */
function resolveSelection(tab: PickerTab, draft: CardDraft, hasSubject: boolean) {
  const stored = draft[tab.slot];
  const listed = stored ? tab.entries.find((entry) => entry.id === stored.id) : undefined;
  if (listed && stored) return { entry: listed, choice: stored };
  const fallback = tab.optional ? undefined : defaultCut(tab.entries, hasSubject);
  return fallback ? { entry: fallback, choice: styleChoice(fallback) } : null;
}

function Editor({
  image,
  mask,
  appVersion,
  remoteConfig,
  registry = shippedRegistry,
  captionMax = CAPTION_MAX_GRAPHEMES,
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
    const tabs = pickerTabs(registry, deviceContext(appVersion, subject !== null), remoteConfig);
    const cut = defaultCut(tabs[0]?.entries ?? [], subject !== null);
    return initialHistory(cut ?? registry.byKind('cutStyle')[0]!);
  });
  const [activeSlot, setActiveSlot] = useState<StyleSlot>('cut');
  const [captionOpen, setCaptionOpen] = useState(false);
  const toast = useToast();

  const draft = history.present;
  const hasSubject = subject !== null && !draft.wholePhoto;
  const tabs = pickerTabs(registry, deviceContext(appVersion, hasSubject), remoteConfig);
  const cutTab = tabs.find((tab) => tab.slot === 'cut')!;
  const cut = resolveSelection(cutTab, draft, hasSubject);
  const finishTab = tabs.find((tab) => tab.slot === 'finish')!;
  const finish = resolveSelection(finishTab, draft, hasSubject);
  const shown: CardDraft | null = cut
    ? {
        cut: cut.choice,
        finish: finish?.choice ?? null,
        wholePhoto: draft.wholePhoto,
        caption: draft.caption,
      }
    : null;

  const scene = useMemo(
    () => ({ width: size.width, height: size.height, photo: image, subject }),
    [size.width, size.height, image, subject],
  );
  const shownKey = JSON.stringify(shown);
  const picture = useMemo(
    () =>
      shown ? createPicture((canvas) => drawCard(canvas, registry, shown, scene), size) : null,
    // `shown` is rebuilt every render; its JSON is the stable identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [shownKey, scene, registry],
  );

  const activeTab = tabs.find((tab) => tab.slot === activeSlot) ?? cutTab;
  const active = activeSlot === 'cut' ? cut : finish;
  const select = (entry: EffectEntry | null) =>
    dispatch(
      activeTab.slot === 'cut' && entry
        ? { type: 'select', slot: 'cut', entry }
        : { type: 'select', slot: 'finish', entry },
    );
  const setParams = (entry: EffectEntry, params: Record<string, unknown>, transient?: boolean) =>
    dispatch({ type: 'setParams', slot: activeTab.slot, entry, params, transient });
  const thumbnails = useTileThumbnails(registry, activeTab, shown, image, mask);
  const tileSize = thumbSize(image.width() / image.height());
  const pressTile = (tile: PickerTile) => {
    if (tile.state.kind === 'available') select(tile.entry);
    else if (tile.state.kind === 'locked') toast.show({ message: t('editor.tile.locked.toast') });
    else toast.show({ message: unavailableReason(tile.state.reason) });
  };

  const previewLabel = cut
    ? finish
      ? t('editor.preview.labelWithFinish', {
          style: t(cut.entry.a11yLabel as MessageKey),
          finish: t(finish.entry.a11yLabel as MessageKey),
        })
      : t('editor.preview.label', { style: t(cut.entry.a11yLabel as MessageKey) })
    : undefined;

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
      <View
        style={[styles.preview, size]}
        accessible
        accessibilityRole="image"
        accessibilityLabel={previewLabel}
        testID="editor-preview"
      >
        {picture ? (
          <Canvas style={size}>
            <Picture picture={picture} />
          </Canvas>
        ) : (
          <Text tone="muted">{t('editor.unavailable')}</Text>
        )}
      </View>
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
        {activeTab.optional ? (
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
            selected={tile.entry.id === active?.entry.id}
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

const styles = StyleSheet.create({
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
