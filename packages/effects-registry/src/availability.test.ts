import { describe, expect, it } from 'vitest';

import {
  compareVersions,
  killedIds,
  resolveAvailability,
  visibleEntries,
  type DeviceContext,
} from './availability';
import { ENTRIES } from './entries';
import { cutDiecut } from './entries/cut.diecut';
import { fxHolo } from './entries/fx.holo';
import type { EffectEntry } from './schema';

const iphone: DeviceContext = {
  platform: 'ios',
  osVersion: '17.4',
  appVersion: '0.1.0',
  capabilities: new Set(['subjectMask', 'gyro']),
};
const noGms: DeviceContext = {
  platform: 'android',
  osVersion: 29,
  appVersion: '0.1.0',
  capabilities: new Set(),
};

describe('resolveAvailability', () => {
  it('shows an entry whose requirements hold', () => {
    expect(resolveAvailability(cutDiecut, iphone)).toEqual({ status: 'available' });
  });

  it('offers the declared fallback when a capability is missing', () => {
    const tapTheEyes: EffectEntry = {
      ...fxHolo,
      platforms: {
        ...fxHolo.platforms,
        android: { min: 24, requires: ['animalPose'], fallback: 'userPoints' },
      },
    };
    expect(resolveAvailability(tapTheEyes, noGms)).toEqual({
      status: 'fallback',
      fallback: 'userPoints',
      missing: ['animalPose'],
    });
  });

  it('hides mask-based cut styles without an on-device mask (no server fallback, AC-7.6)', () => {
    expect(resolveAvailability(cutDiecut, noGms)).toEqual({ status: 'hidden', reason: 'requires' });
  });

  it('hides an entry without a fallback when a capability is missing', () => {
    const poseOnly: EffectEntry = {
      ...fxHolo,
      platforms: { ...fxHolo.platforms, android: { min: 24, requires: ['animalPose'] } },
    };
    expect(resolveAvailability(poseOnly, noGms)).toEqual({ status: 'hidden', reason: 'requires' });
  });

  it('applies kill switches from any config group', () => {
    const config = {
      kill_switches: { cut_styles: [], finishes: [], effects: ['cut.diecut'], reactions: [] },
    };
    expect(resolveAvailability(cutDiecut, iphone, config)).toEqual({
      status: 'hidden',
      reason: 'killSwitch',
    });
    expect(killedIds(config)).toEqual(new Set(['cut.diecut']));
    expect(killedIds(undefined).size).toBe(0);
  });

  it('keeps feature-flagged entries dark until the flag is on', () => {
    const dark: EffectEntry = { ...fxHolo, flags: { featureFlag: 'holo_v2' } };
    expect(resolveAvailability(dark, iphone)).toEqual({ status: 'hidden', reason: 'featureFlag' });
    expect(resolveAvailability(dark, iphone, { flags: { holo_v2: false } }).status).toBe('hidden');
    expect(resolveAvailability(dark, iphone, { flags: { holo_v2: true } })).toEqual({
      status: 'available',
    });
  });

  it('hides entries newer than the app or the OS', () => {
    const newer: EffectEntry = { ...fxHolo, flags: { minAppVersion: '0.10.0' } };
    expect(resolveAvailability(newer, iphone)).toEqual({
      status: 'hidden',
      reason: 'minAppVersion',
    });
    const ios18: EffectEntry = {
      ...fxHolo,
      platforms: { ...fxHolo.platforms, ios: { min: '18', requires: [] } },
    };
    expect(resolveAvailability(ios18, iphone)).toEqual({ status: 'hidden', reason: 'os' });
  });
});

describe('visibleEntries', () => {
  it('lists available entries in registry order, minus killed ones', () => {
    const shown = visibleEntries(ENTRIES, iphone, { kill_switches: { finishes: ['fx.holo'] } });
    expect(shown.map(({ entry }) => entry.id)).toEqual([
      'cut.none',
      'cut.raw',
      'cut.diecut',
      'fx.paper-lift',
    ]);
    expect(visibleEntries(ENTRIES, noGms).map(({ entry }) => entry.id)).toEqual([
      'cut.none',
      'fx.holo',
      'fx.paper-lift',
    ]);
  });
});

describe('compareVersions', () => {
  it.each([
    ['17.4', '17', 1],
    ['16.4', '17', -1],
    ['0.10.0', '0.9.1', 1],
    [34, 24, 1],
    ['1.0.0', '1', 0],
  ] as const)('%s vs %s', (a, b, expected) => {
    expect(compareVersions(a, b)).toBe(expected);
  });
});
