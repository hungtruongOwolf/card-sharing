/**
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { Skia } from '@shopify/react-native-skia';
import { act, fireEvent, screen, userEvent } from '@testing-library/react-native';
import { Platform, StyleSheet } from 'react-native';
import { State } from 'react-native-gesture-handler';
import { fireGestureHandler, getByGestureTestId } from 'react-native-gesture-handler/jest-utils';

import { createRegistry, ENTRIES, registry } from '@card-sharing/effects-registry';

import { t } from '../../../i18n';
import { renderThemed } from '../../../test/render';
import type { CapturedPhoto } from '../../capture';
import { CardEditorScreen, type CardEditorScreenProps } from '../CardEditorScreen';
import { createSyntheticMaskSource, noMaskSource } from '../maskSource';
import { sliderValue } from '../ParamControls';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

// jest-expo leaves Platform.Version undefined; registry routing compares it with each entry's minimum OS.
beforeAll(() => {
  Object.defineProperty(Platform, 'Version', { value: '17.4', configurable: true });
});

// Renders with CanvasKit WASM: slow under a parallel full run.
const RENDER_TIMEOUT_MS = 30_000;

const photo: CapturedPhoto = {
  uri: 'file:///cache/photo.jpg',
  width: 300,
  height: 400,
  origin: 'library',
};

async function loadImage() {
  const surface = Skia.Surface.Make(photo.width, photo.height)!;
  surface.getCanvas().clear(Skia.Color('#3366CC'));
  surface.flush();
  return surface.makeImageSnapshot();
}

async function renderEditor(
  props: Partial<CardEditorScreenProps> = {},
  options: { reducedMotion?: boolean } = {},
) {
  await renderThemed(
    <CardEditorScreen
      photo={photo}
      maskSource={createSyntheticMaskSource()}
      appVersion="0.1.0"
      loadImage={loadImage}
      onBack={jest.fn()}
      {...props}
    />,
    options,
  );
  return screen.findByTestId('editor');
}

const tile = (key: Parameters<typeof t>[0]) => screen.queryByRole('button', { name: t(key) });
const selected = (key: Parameters<typeof t>[0]) =>
  screen.getByRole('button', { name: t(key) }).props.accessibilityState?.selected;

describe('CardEditorScreen', () => {
  it(
    'starts as a sticker, edits params, and undoes and redoes the changes',
    async () => {
      const onNext = jest.fn();
      await renderEditor({ onNext });
      const user = userEvent.setup();
      expect(selected('effects.cut.diecut.name')).toBe(true);
      expect(
        screen.getByLabelText(t('editor.preview.label', { style: t('effects.cut.diecut.a11y') })),
      ).toBeTruthy();

      await user.press(screen.getByRole('button', { name: t('effects.cut.diecut.preset.thick') }));
      const border = t('effects.param.borderWidthPct');
      const stepper = screen.getByRole('adjustable', { name: border });
      expect(stepper.props.accessibilityValue).toMatchObject({ now: 6, max: 6 });
      await fireEvent(stepper, 'accessibilityAction', { nativeEvent: { actionName: 'decrement' } });
      expect(screen.getByRole('adjustable', { name: border }).props.accessibilityValue.now).toBe(
        5.725,
      );

      await user.press(screen.getByRole('button', { name: t('editor.undo') }));
      expect(screen.getByRole('adjustable', { name: border }).props.accessibilityValue.now).toBe(6);
      await user.press(screen.getByRole('button', { name: t('editor.undo') }));
      expect(screen.getByRole('adjustable', { name: border }).props.accessibilityValue.now).toBe(3);
      expect(screen.getByRole('button', { name: t('editor.undo') })).toBeDisabled();
      await user.press(screen.getByRole('button', { name: t('editor.redo') }));
      expect(screen.getByRole('adjustable', { name: border }).props.accessibilityValue.now).toBe(6);

      await user.press(screen.getByRole('button', { name: t('editor.caption.add') }));
      await user.type(screen.getByTestId('caption-input'), 'zoomies \u{1F431}\u202E');
      await user.press(screen.getByRole('button', { name: t('caption.save') }));
      expect(screen.getByTestId('editor-caption')).toHaveTextContent('zoomies \u{1F431}');
      expect(screen.getByRole('button', { name: t('editor.caption.edit') })).toBeTruthy();

      await user.press(screen.getByRole('radio', { name: t('editor.color.pink') }));
      expect(
        screen.getByRole('radio', { name: t('editor.color.pink') }).props.accessibilityState,
      ).toMatchObject({ checked: true });
      await user.press(screen.getByRole('button', { name: t('editor.next') }));
      expect(onNext).toHaveBeenCalledWith({
        cut: { id: 'cut.diecut', version: 1, params: { borderWidthPct: 6, color: '#F7A1C4' } },
        finish: null,
        effects: [],
        wholePhoto: false,
        caption: 'zoomies \u{1F431}',
      });
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'picks a finish from the finish tab and labels the preview with it',
    async () => {
      await renderEditor();
      const user = userEvent.setup();
      await user.press(screen.getByRole('tab', { name: t('editor.tab.finish') }));
      expect(selected('editor.finish.none')).toBe(true);
      expect(tile('effects.fx.paperLift.name')).toBeNull();
      await user.press(screen.getByRole('button', { name: t('effects.fx.holo.name') }));
      expect(
        screen.getByLabelText(
          t('editor.preview.labelWithFinish', {
            style: t('effects.cut.diecut.a11y'),
            finish: t('effects.fx.holo.a11y'),
          }),
        ),
      ).toBeTruthy();
      expect(screen.getByRole('adjustable', { name: t('effects.param.intensity') })).toBeTruthy();
      // The focused card gets the live gyro shine.
      expect(screen.getByTestId('editor-live-finish')).toBeTruthy();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'reopens a saved card with its choices (re-editable until sent)',
    async () => {
      const onNext = jest.fn();
      const onDraftChange = jest.fn();
      const holo = registry.get('fx.holo')!;
      const torn = registry.get('cut.torn')!;
      await renderEditor(
        {
          onNext,
          onDraftChange,
          initialDraft: {
            cut: {
              id: torn.id,
              version: torn.version,
              params: { roughness: 0.5, seed: 7, paper: 'kraft' },
            },
            finish: { id: holo.id, version: holo.version, params: { intensity: 0.4 } },
            effects: [],
            wholePhoto: false,
            caption: 'again',
          },
        },
        { reducedMotion: true },
      );
      expect(selected('effects.cut.torn.name')).toBe(true);
      expect(screen.getByTestId('editor-caption')).toHaveTextContent('again');
      // Auto-save hook (drafts tray, T-68) sees the shown draft.
      expect(onDraftChange).toHaveBeenLastCalledWith(
        expect.objectContaining({
          caption: 'again',
          cut: expect.objectContaining({ id: 'cut.torn' }),
        }),
      );
      await userEvent.setup().press(screen.getByRole('button', { name: t('editor.next') }));
      expect(onNext).toHaveBeenCalledWith(
        expect.objectContaining({
          cut: expect.objectContaining({
            id: 'cut.torn',
            params: expect.objectContaining({ paper: 'kraft' }),
          }),
          finish: expect.objectContaining({ id: 'fx.holo', params: { intensity: 0.4 } }),
        }),
      );
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'offers matte, glossy and holo, and keeps holo static under reduced motion',
    async () => {
      await renderEditor({}, { reducedMotion: true });
      const user = userEvent.setup();
      await user.press(screen.getByRole('tab', { name: t('editor.tab.finish') }));
      for (const key of [
        'effects.fx.matte.name',
        'effects.fx.glossy.name',
        'effects.fx.holo.name',
      ] as const) {
        expect(tile(key)).toBeTruthy();
      }
      await user.press(screen.getByRole('button', { name: t('effects.fx.holo.name') }));
      expect(selected('effects.fx.holo.name')).toBe(true);
      expect(screen.queryByTestId('editor-live-finish')).toBeNull();
      await user.press(screen.getByRole('button', { name: t('effects.fx.matte.name') }));
      expect(
        screen.getByLabelText(
          t('editor.preview.labelWithFinish', {
            style: t('effects.cut.diecut.a11y'),
            finish: t('effects.fx.matte.a11y'),
          }),
        ),
      ).toBeTruthy();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'offers only mask-free styles for a whole-photo card',
    async () => {
      await renderEditor();
      const user = userEvent.setup();
      await fireEvent(screen.getByLabelText(t('editor.wholePhoto')), 'valueChange', true);
      expect(tile('effects.cut.diecut.name')).toBeNull();
      expect(tile('effects.cut.raw.name')).toBeNull();
      expect(screen.queryByTestId('tile-cut.diecut')).toBeNull();
      expect(selected('effects.cut.none.name')).toBe(true);
      await user.press(screen.getByRole('button', { name: t('editor.undo') }));
      expect(selected('effects.cut.diecut.name')).toBe(true);
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'makes whole-photo cards on devices without cutout and hides killed styles',
    async () => {
      await renderEditor({
        maskSource: noMaskSource,
        remoteConfig: { kill_switches: { finishes: ['fx.holo'] } },
      });
      expect(screen.getByText(t('editor.noSubject'))).toBeTruthy();
      expect(screen.queryByLabelText(t('editor.wholePhoto'))).toBeNull();
      expect(tile('effects.cut.diecut.name')).toBeNull();
      expect(selected('effects.cut.none.name')).toBe(true);
      // Mask-free shape cuts for whole-photo cards (EM decision 2026-11-05, T-63).
      for (const key of [
        'effects.cut.torn.name',
        'effects.cut.shape.roundedSquare.name',
        'effects.cut.shape.circle.name',
        'effects.cut.shape.heart.name',
      ] as const) {
        expect(tile(key)).toBeTruthy();
      }
      await userEvent
        .setup()
        .press(screen.getByRole('button', { name: t('effects.cut.torn.name') }));
      expect(screen.getByRole('radio', { name: t('effects.param.paper.kraft') })).toBeTruthy();
      // The seed is a hidden param: roughness is the only stepper.
      expect(
        screen.getAllByRole('adjustable').map((node) => node.props.accessibilityLabel),
      ).toEqual([t('effects.param.roughness')]);
      await userEvent.setup().press(screen.getByRole('tab', { name: t('editor.tab.finish') }));
      expect(tile('effects.fx.holo.name')).toBeNull();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'drags a slider as one undo step',
    async () => {
      await renderEditor();
      const user = userEvent.setup();
      const border = t('effects.param.borderWidthPct');
      const spec = registry.get('cut.diecut')!.params.borderWidthPct!;
      if (spec.type !== 'number') throw new Error('expected a number param');
      const value = () =>
        screen.getByRole('adjustable', { name: border }).props.accessibilityValue.now as number;
      const before = value();
      const id = `param-slider-${border}`;
      await fireEvent(screen.getByTestId(id, { includeHiddenElements: true }), 'layout', {
        nativeEvent: { layout: { x: 0, y: 0, width: 200, height: 44 } },
      });
      await act(async () => {
        fireGestureHandler(getByGestureTestId(id), [
          { state: State.BEGAN, x: 10 },
          { state: State.ACTIVE, x: 60 },
          { x: 120 },
          { x: 150 },
          { state: State.END, x: 150 },
        ]);
      });
      expect(value()).toBe(sliderValue(spec, 0.75));
      await user.press(screen.getByRole('button', { name: t('editor.undo') }));
      expect(value()).toBe(before);
      expect(screen.getByRole('button', { name: t('editor.undo') })).toBeDisabled();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'explains locked and unavailable tiles instead of applying them',
    async () => {
      const custom = createRegistry(
        ENTRIES.map((e) => {
          if (e.id === 'cut.raw') return { ...e, unlock: 'pro' as const };
          if (e.id === 'cut.torn') return { ...e, flags: { minAppVersion: '9.0.0' } };
          return e;
        }),
      );
      await renderEditor({ registry: custom });
      const user = userEvent.setup();
      const reason = t('editor.tile.reason.needsUpdate');
      const torn = screen.getByRole('button', {
        name: t('editor.tile.unavailable.a11y', { name: t('effects.cut.torn.name'), reason }),
      });
      expect(torn.props.accessibilityState).toMatchObject({ disabled: true, selected: false });
      expect(screen.getByText(reason)).toBeTruthy();
      const raw = screen.getByRole('button', {
        name: t('editor.tile.locked.a11y', { name: t('effects.cut.raw.name'), unlock: 'pro' }),
      });
      await fireEvent.press(raw);
      expect(screen.getByText(t('editor.tile.locked.toast'))).toBeTruthy();
      expect(selected('effects.cut.diecut.name')).toBe(true);
      expect(screen.getByRole('button', { name: t('editor.undo') })).toBeDisabled();
      await user.press(screen.getByRole('button', { name: t('effects.cut.none.name') }));
      expect(selected('effects.cut.none.name')).toBe(true);
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'stacks effects only on a tap, replaces conflicting ones and removes them (T-114)',
    async () => {
      const glossy = registry.get('fx.glossy')!;
      const matte = registry.get('fx.matte')!;
      // Effect-slot filters (no `finish` tag) that refuse each other.
      const sheen = { ...glossy, id: 'fx.sheen', tags: [], conflictsWith: ['fx.grain'] };
      const grain = { ...matte, id: 'fx.grain', tags: [], stackable: true };
      const custom = createRegistry([...ENTRIES, sheen, grain]);
      const onNext = jest.fn();
      const onDraftChange = jest.fn();
      await renderEditor({ registry: custom, onNext, onDraftChange });
      const user = userEvent.setup();
      // AC-21.4: nothing is applied until the user picks it.
      expect(onDraftChange).toHaveBeenLastCalledWith(expect.objectContaining({ effects: [] }));
      await user.press(screen.getByRole('tab', { name: t('editor.tab.effects') }));
      expect(selected('effects.fx.glossy.name')).toBe(false);

      await user.press(screen.getByRole('button', { name: t('effects.fx.glossy.name') }));
      expect(selected('effects.fx.glossy.name')).toBe(true);
      const intensity = screen.getByRole('adjustable', { name: t('effects.param.intensity') });
      await fireEvent(intensity, 'accessibilityAction', {
        nativeEvent: { actionName: 'decrement' },
      });
      expect(
        screen.getByLabelText(
          t('editor.preview.labelWithEffects', {
            label: t('editor.preview.label', { style: t('effects.cut.diecut.a11y') }),
            effects: t('effects.fx.glossy.a11y'),
          }),
        ),
      ).toBeTruthy();

      await user.press(screen.getByRole('button', { name: t('effects.fx.matte.name') }));
      expect(
        screen.getByText(
          t('editor.effects.replaced', {
            name: t('effects.fx.matte.name'),
            replaced: t('effects.fx.glossy.name'),
          }),
        ),
      ).toBeTruthy();
      expect(selected('effects.fx.glossy.name')).toBe(false);
      expect(selected('effects.fx.matte.name')).toBe(true);
      await user.press(screen.getByRole('button', { name: t('editor.effects.addAnother') }));
      expect(onDraftChange).toHaveBeenLastCalledWith(
        expect.objectContaining({
          effects: [
            { id: 'fx.grain', version: 1, params: { intensity: 0.6 } },
            { id: 'fx.grain', version: 1, params: { intensity: 0.6 } },
          ],
        }),
      );
      await user.press(
        screen.getByRole('button', {
          name: t('editor.effects.remove', { name: t('effects.fx.matte.name') }),
        }),
      );
      await user.press(screen.getByRole('button', { name: t('editor.undo') }));
      await user.press(screen.getByRole('button', { name: t('editor.undo') }));
      await user.press(screen.getByRole('button', { name: t('editor.next') }));
      expect(onNext).toHaveBeenLastCalledWith(
        expect.objectContaining({
          effects: [{ id: 'fx.grain', version: 1, params: { intensity: 0.6 } }],
        }),
      );
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'places a speech bubble on the subject and lets the user drag it and aim its tail (T-119)',
    async () => {
      const onDraftChange = jest.fn();
      const anchorSource = { anchorsFor: jest.fn(async () => null) };
      await renderEditor({ onDraftChange, anchorSource, loadFonts: async () => undefined });
      expect(anchorSource.anchorsFor).toHaveBeenCalledWith(photo);
      const user = userEvent.setup();
      await user.press(screen.getByRole('tab', { name: t('editor.tab.effects') }));
      await user.press(screen.getByRole('button', { name: t('effects.overlay.speech.name') }));
      const speech = () => onDraftChange.mock.lastCall[0].effects[0].params;
      // No native landmarks: the tail aims at the subject's head band (mask heuristic), the bubble sits beside it.
      const placed = speech();
      expect(placed.tailX).toBeCloseTo(0.5, 1);
      expect(placed.tailY).toBeGreaterThan(0.1);
      expect(placed.tailY).toBeLessThan(0.5);
      expect(screen.getByText(t('editor.place.hint'))).toBeTruthy();

      await act(async () => {
        fireGestureHandler(getByGestureTestId('editor-place'), [
          { state: State.BEGAN, translationX: 0, translationY: 0 },
          { state: State.ACTIVE, translationX: 10, translationY: 0 },
          { translationX: 30, translationY: 20 },
          { state: State.END, translationX: 30, translationY: 20 },
        ]);
      });
      const preview = screen.getByTestId('editor-preview');
      const { width } = StyleSheet.flatten(preview.props.style) as { width: number };
      expect(speech().x).toBeCloseTo(placed.x + 30 / width, 3);
      expect(speech().tailX).toBe(placed.tailX);
      // One drag, one undo step.
      await user.press(screen.getByRole('button', { name: t('editor.undo') }));
      expect(speech()).toEqual(placed);

      await user.press(screen.getByRole('radio', { name: t('editor.place.tail') }));
      await fireEvent(preview, 'accessibilityAction', { nativeEvent: { actionName: 'move-up' } });
      expect(speech().tailY).toBeCloseTo(placed.tailY - 0.05);
      expect(speech().x).toBe(placed.x);
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'hides the Effects tab when no effect can be listed',
    async () => {
      // Every shipped effect switched off remotely.
      const effects = ENTRIES.filter(
        (e) => e.kind === 'frame' || e.kind === 'overlay' || e.kind === 'prop',
      );
      await renderEditor({
        remoteConfig: { kill_switches: { effects: effects.map((e) => e.id) } },
      });
      expect(screen.queryByRole('tab', { name: t('editor.tab.effects') })).toBeNull();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'shows baked thumbnails on the tiles',
    async () => {
      await renderEditor();
      const thumb = await screen.findByTestId('tile-thumb-cut.diecut');
      expect(thumb.props.source).toEqual({
        uri: expect.stringMatching(/^data:image\/png;base64,/),
      });
      expect(screen.getByTestId('tile-thumb-cut.none')).toBeTruthy();
    },
    RENDER_TIMEOUT_MS,
  );

  it('falls back to the whole photo when segmentation fails', async () => {
    await renderEditor({ maskSource: { maskFor: () => Promise.reject(new Error('boom')) } });
    expect(screen.getByText(t('editor.noSubject'))).toBeTruthy();
  });

  it('shows an error with a way back when the photo cannot be decoded', async () => {
    const onBack = jest.fn();
    await renderThemed(
      <CardEditorScreen
        photo={photo}
        maskSource={noMaskSource}
        appVersion="0.1.0"
        loadImage={async () => null}
        onBack={onBack}
      />,
    );
    await userEvent
      .setup()
      .press(await screen.findByRole('button', { name: t('editor.error.back') }));
    expect(onBack).toHaveBeenCalled();
  });
});
