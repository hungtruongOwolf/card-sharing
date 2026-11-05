/**
 * Picker routing reads the cut-style and finish renderers, which load Skia (CanvasKit WASM here).
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { Platform } from 'react-native';

import { maskToContours } from '@card-sharing/canvas-engine';
import { createRegistry, ENTRIES, registry } from '@card-sharing/effects-registry';

import { editorParams, parseEditorPhoto } from '../editorRoute';
import { createSyntheticMaskSource, maskSizeFor, noMaskSource } from '../maskSource';
import { defaultCut, deviceContext, pickerTabs } from '../pickerTabs';

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

const ids = (entries: readonly { id: string }[]) => entries.map((entry) => entry.id);
const tabIds = (hasSubject: boolean, config?: Parameters<typeof pickerTabs>[2]) =>
  Object.fromEntries(
    pickerTabs(registry, deviceContext('0.1.0', hasSubject), config).map((tab) => [
      tab.slot,
      ids(tab.entries),
    ]),
  );

describe('picker tabs', () => {
  it('lists mask-based cut styles only when the card has a subject', () => {
    expect(tabIds(true).cut).toEqual(expect.arrayContaining(['cut.none', 'cut.raw', 'cut.diecut']));
    expect(tabIds(false).cut).not.toEqual(expect.arrayContaining(['cut.raw']));
    expect(tabIds(false).cut).not.toContain('cut.diecut');
    expect(tabIds(false).cut).toContain('cut.none');
  });

  it('lists finishes but never built-in entries such as paper lift', () => {
    expect(tabIds(true).finish).toEqual(['fx.holo']);
  });

  it('respects remote kill switches and feature flags', () => {
    const killed = tabIds(true, {
      kill_switches: { cut_styles: ['cut.diecut'], finishes: ['fx.holo'] },
    });
    expect(killed.cut).not.toContain('cut.diecut');
    expect(killed.finish).toEqual([]);
    const dark = createRegistry(
      ENTRIES.map((e) => (e.id === 'cut.raw' ? { ...e, flags: { featureFlag: 'raw_v2' } } : e)),
    );
    const cutIds = (flags: Record<string, boolean>) =>
      ids(pickerTabs(dark, deviceContext('0.1.0', true), { flags })[0]!.entries);
    expect(cutIds({})).not.toContain('cut.raw');
    expect(cutIds({ raw_v2: true })).toContain('cut.raw');
  });

  it('defaults to a sticker-style cut with a subject and the first style without one', () => {
    const cuts = registry.byKind('cutStyle');
    expect(defaultCut(cuts, true)?.inputs).toContain('contour');
    expect(defaultCut(cuts, false)).toBe(cuts[0]);
    expect(defaultCut([], true)).toBeUndefined();
  });
});

describe('mask sources', () => {
  const photo = {
    uri: 'file:///cache/a.jpg',
    width: 3000,
    height: 4000,
    origin: 'camera',
  } as const;

  it('size the mask with the photo aspect ratio', () => {
    expect(maskSizeFor(photo)).toEqual({ width: 192, height: 256 });
    expect(maskSizeFor({ width: 4000, height: 3000 })).toEqual({ width: 256, height: 192 });
    expect(maskSizeFor({ width: 0, height: 0 })).toEqual({ width: 256, height: 256 });
  });

  it('give a synthetic subject in development and none on devices without cutout', async () => {
    const mask = await createSyntheticMaskSource().maskFor(photo);
    expect(mask).not.toBeNull();
    const { contours, bounds } = maskToContours(mask!);
    expect(contours.filter((c) => !c.hole)).toHaveLength(1);
    expect(bounds!.width).toBeLessThan(mask!.width);
    expect(await noMaskSource.maskFor(photo)).toBeNull();
  });
});

describe('editor route params', () => {
  const photo = { uri: 'file:///cache/a.jpg', width: 300, height: 400, origin: 'library' } as const;

  it('round-trip a captured photo', () => {
    expect(parseEditorPhoto(editorParams(photo))).toEqual(photo);
  });

  it.each([
    ['a remote URL', { uri: 'https://evil.example/a.jpg' }],
    ['a content URI', { uri: 'content://media/1' }],
    ['a zero width', { width: '0' }],
    ['a huge height', { height: '99999' }],
    ['a fractional width', { width: '1.5' }],
    ['an unknown origin', { origin: 'web' }],
    ['an array value', { uri: ['file:///a.jpg'] }],
  ])('reject %s', (_label, change) => {
    expect(parseEditorPhoto({ ...editorParams(photo), ...change })).toBeNull();
  });
});
