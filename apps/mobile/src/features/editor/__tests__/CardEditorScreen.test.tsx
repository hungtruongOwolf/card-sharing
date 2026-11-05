/**
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { Skia } from '@shopify/react-native-skia';
import { fireEvent, screen, userEvent } from '@testing-library/react-native';
import { Platform } from 'react-native';

import { t } from '../../../i18n';
import { renderThemed } from '../../../test/render';
import type { CapturedPhoto } from '../../capture';
import { CardEditorScreen, type CardEditorScreenProps } from '../CardEditorScreen';
import { createSyntheticMaskSource, noMaskSource } from '../maskSource';

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

async function renderEditor(props: Partial<CardEditorScreenProps> = {}) {
  await renderThemed(
    <CardEditorScreen
      photo={photo}
      maskSource={createSyntheticMaskSource()}
      appVersion="0.1.0"
      loadImage={loadImage}
      onBack={jest.fn()}
      {...props}
    />,
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
