/**
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { Skia, type SkImage } from '@shopify/react-native-skia';
import { act, fireEvent, screen } from '@testing-library/react-native';
import { State } from 'react-native-gesture-handler';
import { fireGestureHandler, getByGestureTestId } from 'react-native-gesture-handler/jest-utils';
import { makeMutable } from 'react-native-reanimated';

import type { Camera } from '@card-sharing/canvas-engine';
import { registry } from '@card-sharing/effects-registry';

import { t } from '../../../i18n';
import { cardItem, holo, pin } from '../../../test/boardFixtures';
import { renderThemed, SCHEMES } from '../../../test/render';
import { BoardCanvas } from '../BoardCanvas';
import { createTextureCache } from '../textureCache';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const RENDER_TIMEOUT_MS = 30_000;

function texture(): SkImage {
  const surface = Skia.Surface.Make(32, 32)!;
  surface.getCanvas().clear(Skia.Color('#3366CC'));
  surface.flush();
  return surface.makeImageSnapshot();
}

const shown = pin({ x: 150, y: 200 }, { kind: 'card', card: cardItem() });
const tile = pin({ x: 300, y: 500, zKey: 'a1' }, { kind: 'private_tile' });
const gone = pin({ x: 120, y: 600, zKey: 'a2' }, { kind: 'removed' });
const offscreen = pin({ x: 9000, y: 9000, zKey: 'a3' });
const shiny = pin(
  { x: 300, y: 250, zKey: 'a4' },
  { kind: 'card', card: cardItem({ finish: holo(), sender: { id: 'u2', displayName: null } }) },
);

interface EditProps {
  editable?: boolean;
  selectedId?: string | null;
  onSelect?: jest.Mock;
  onPickUp?: jest.Mock;
  onCommit?: jest.Mock;
  onAction?: jest.Mock;
}

async function renderBoard(
  options: { reducedMotion?: boolean; scheme?: 'light' | 'dark'; camera?: Camera } = {},
  edit: EditProps = {},
) {
  const load = jest.fn(async () => texture());
  const textures = createTextureCache({ load });
  const camera = makeMutable<Camera>(options.camera ?? { x: 0, y: 0, zoom: 1 });
  await renderThemed(
    <BoardCanvas
      pins={[shown, tile, gone, offscreen, shiny]}
      width={400}
      height={800}
      textures={textures}
      initialCamera={camera.get()}
      camera={camera}
      registry={registry}
      {...edit}
    />,
    options,
  );
  await act(async () => {});
  return { load, camera, textures };
}

describe('BoardCanvas', () => {
  it.each(SCHEMES)(
    'labels every pin on screen for screen readers in %s mode, never off-screen ones',
    async (scheme) => {
      await renderBoard({ scheme });
      expect(screen.getByLabelText(t('boards.pin.card', { name: 'Mai' }))).toBeTruthy();
      expect(screen.getByLabelText(t('boards.pin.card.someone'))).toBeTruthy();
      expect(screen.getByLabelText(t('boards.pin.private'))).toBeTruthy();
      expect(screen.getByLabelText(t('boards.pin.removed'))).toBeTruthy();
      expect(screen.queryByTestId(`board-pin-${offscreen.id}`)).toBeNull();
      expect(screen.getByTestId('board-canvas').props.accessibilityLabel).toBe(
        t('boards.canvas.label', { count: 4 }),
      );
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'loads textures only for visible card pins, keyed by card id',
    async () => {
      const { load, textures } = await renderBoard();
      expect(load).toHaveBeenCalledTimes(2);
      const urls = load.mock.calls.map((call) => (call as unknown[])[0]);
      expect(urls).not.toContain(
        offscreen.content.kind === 'card' ? offscreen.content.card.media!.renderUrl : '',
      );
      expect(textures.get(shown.content.kind === 'card' ? shown.content.card.id : '')).toBeTruthy();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'pans and zooms the camera with gestures and re-labels pins where they now are',
    async () => {
      const { camera } = await renderBoard();
      await act(async () => {
        fireGestureHandler(getByGestureTestId('board-canvas-pan'), [
          { state: State.BEGAN, translationX: 0, translationY: 0 },
          { state: State.ACTIVE, translationX: 0, translationY: 0 },
          { translationX: -100, translationY: 0 },
          { state: State.END, translationX: -100, translationY: 0 },
        ]);
      });
      expect(camera.get().x).toBeCloseTo(100);
      const label = screen.getByTestId(`board-pin-${shown.id}`);
      expect(label.props.style.start).toBeLessThan(100);

      await act(async () => {
        fireGestureHandler(getByGestureTestId('board-canvas-pinch'), [
          { state: State.BEGAN, scale: 1, focalX: 0, focalY: 0 },
          { state: State.ACTIVE, scale: 1, focalX: 0, focalY: 0 },
          { scale: 2, focalX: 0, focalY: 0 },
          { state: State.END, scale: 2, focalX: 0, focalY: 0 },
        ]);
      });
      expect(camera.get().zoom).toBeCloseTo(2);
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'picks up, moves and commits a pin dragged in edit mode, leaving the camera alone',
    async () => {
      const onPickUp = jest.fn();
      const onCommit = jest.fn();
      const { camera } = await renderBoard({}, { editable: true, onPickUp, onCommit });
      await act(async () => {
        fireGestureHandler(getByGestureTestId('board-canvas-pan'), [
          { state: State.BEGAN, x: 150, y: 200, translationX: 0, translationY: 0 },
          { state: State.ACTIVE, x: 150, y: 200, translationX: 0, translationY: 0 },
          { x: 200, y: 230, translationX: 50, translationY: 30 },
          { state: State.END, x: 200, y: 230, translationX: 50, translationY: 30 },
        ]);
      });
      expect(onPickUp).toHaveBeenCalledWith(shown.id);
      expect(onCommit).toHaveBeenCalledWith(shown.id, { x: 200, y: 230, scale: 1, rotation: 0 });
      expect(camera.get()).toEqual({ x: 0, y: 0, zoom: 1 });
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'scales within limits and snaps rotation to a detent on release',
    async () => {
      const onCommit = jest.fn();
      await renderBoard({}, { editable: true, onCommit });
      await act(async () => {
        fireGestureHandler(getByGestureTestId('board-canvas-rotation'), [
          { state: State.BEGAN, rotation: 0, anchorX: 150, anchorY: 200 },
          { state: State.ACTIVE, rotation: 0, anchorX: 150, anchorY: 200 },
          { rotation: (88 * Math.PI) / 180, anchorX: 150, anchorY: 200 },
          { state: State.END, rotation: (88 * Math.PI) / 180, anchorX: 150, anchorY: 200 },
        ]);
      });
      expect(onCommit).toHaveBeenLastCalledWith(
        shown.id,
        expect.objectContaining({ rotation: 90 }),
      );
      await act(async () => {
        fireGestureHandler(getByGestureTestId('board-canvas-pinch'), [
          { state: State.BEGAN, scale: 1, focalX: 150, focalY: 200 },
          { state: State.ACTIVE, scale: 1, focalX: 150, focalY: 200 },
          { scale: 20, focalX: 150, focalY: 200 },
          { state: State.END, scale: 20, focalX: 150, focalY: 200 },
        ]);
      });
      expect(onCommit).toHaveBeenLastCalledWith(shown.id, expect.objectContaining({ scale: 4 }));
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'selects with a tap and clears the selection on the empty board',
    async () => {
      const onSelect = jest.fn();
      await renderBoard({}, { editable: true, onSelect });
      const tap = (x: number, y: number) =>
        act(async () => {
          fireGestureHandler(getByGestureTestId('board-canvas-tap'), [
            { state: State.BEGAN, x, y },
            { state: State.ACTIVE, x, y },
            { state: State.END, x, y },
          ]);
        });
      await tap(150, 200);
      expect(onSelect).toHaveBeenLastCalledWith(shown.id);
      await tap(20, 780);
      expect(onSelect).toHaveBeenLastCalledWith(null);
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'offers move, rotate, scale, layer and remove as screen-reader actions in edit mode only',
    async () => {
      const onAction = jest.fn();
      await renderBoard({}, { editable: true, onAction, selectedId: shown.id });
      const view = screen.getByTestId(`board-pin-${shown.id}`);
      expect(view.props.accessibilityActions.map((a: { name: string }) => a.name)).toEqual([
        'moveUp',
        'moveDown',
        'moveLeft',
        'moveRight',
        'rotateLeft',
        'rotateRight',
        'bigger',
        'smaller',
        'bringToFront',
        'sendToBack',
        'remove',
      ]);
      expect(view.props.accessibilityState).toEqual({ selected: true });
      await fireEvent(view, 'accessibilityAction', { nativeEvent: { actionName: 'bringToFront' } });
      await fireEvent(view, 'accessibilityAction', { nativeEvent: { actionName: 'unknown' } });
      expect(onAction).toHaveBeenCalledTimes(1);
      expect(onAction).toHaveBeenCalledWith(shown.id, 'bringToFront');
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'does not move pins or offer actions when not editable',
    async () => {
      const onCommit = jest.fn();
      const { camera } = await renderBoard({}, { onCommit });
      expect(
        screen.getByTestId(`board-pin-${shown.id}`).props.accessibilityActions,
      ).toBeUndefined();
      await act(async () => {
        fireGestureHandler(getByGestureTestId('board-canvas-pan'), [
          { state: State.BEGAN, x: 150, y: 200, translationX: 0, translationY: 0 },
          { state: State.ACTIVE, x: 150, y: 200, translationX: 0, translationY: 0 },
          { x: 200, y: 200, translationX: 50, translationY: 0 },
          { state: State.END, x: 200, y: 200, translationX: 50, translationY: 0 },
        ]);
      });
      expect(onCommit).not.toHaveBeenCalled();
      expect(camera.get().x).toBeCloseTo(-50);
    },
    RENDER_TIMEOUT_MS,
  );
});
