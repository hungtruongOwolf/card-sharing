import { describe, expect, it } from 'vitest';

import {
  boxShadow,
  cssVariables,
  DEFAULT_SCHEME,
  elevation,
  gesture,
  linearGradient,
  motion,
  spring,
  space,
  themes,
  tokenTable,
  type as typeScale,
  typeStyle,
  MAX_FONT_SCALE,
} from './index';

const COLOR = /^(#[0-9A-F]{6}|rgba?\(\d+, \d+, \d+(, [\d.]+)?\))$/i;

describe('tokens', () => {
  it('defines the same semantic roles for every color scheme', () => {
    const names = (scheme: 'light' | 'dark') =>
      tokenTable()
        .map((row) => [row.name, typeof row[scheme]])
        .sort();
    expect(names('dark')).toEqual(names('light'));
    expect(Object.keys(themes.dark).sort()).toEqual(Object.keys(themes.light).sort());
  });

  it('uses hex or rgb(a) strings for every colour token', () => {
    for (const row of tokenTable()) {
      expect(row.light).toMatch(COLOR);
      expect(row.dark).toMatch(COLOR);
    }
  });

  it('matches the design-system v2 anchors (night canvas, pink primary, dark first)', () => {
    expect(themes.light.bg.canvas).toBe('#FFF7F3');
    expect(themes.dark.bg.canvas).toBe('#0D0A12');
    expect(themes.light.primary.base).toBe('#D61F69');
    expect(themes.dark.primary.base).toBe('#FF4F8B');
    expect(DEFAULT_SCHEME).toBe('dark');
  });

  it('builds CSS / React Native linear gradients with evenly spaced stops', () => {
    expect(linearGradient('primary')).toBe(
      'linear-gradient(120deg, #FF8A5B 0%, #FF4F8B 50%, #B57BFF 100%)',
    );
    expect(linearGradient('scrimTop')).toBe(
      'linear-gradient(180deg, rgba(0, 0, 0, 0.55) 0%, rgba(0, 0, 0, 0) 100%)',
    );
  });

  it('keeps springs physical and the story timing sane', () => {
    for (const config of Object.values(spring)) {
      expect(config.damping).toBeGreaterThan(0);
      expect(config.stiffness).toBeGreaterThan(0);
      expect(config.mass).toBeGreaterThan(0);
    }
    expect(motion.story.holdMs).toBeLessThan(motion.story.segmentMs);
    expect(gesture.backZone).toBeGreaterThan(0);
    expect(gesture.backZone).toBeLessThan(0.5);
  });

  it('keeps the space scale on the 4 pt grid and strictly increasing', () => {
    const values = Object.values(space);
    for (let i = 1; i < values.length; i++) {
      expect(values[i]).toBeGreaterThan(values[i - 1] ?? -1);
      expect((values[i] ?? 1) % 4).toBe(0);
    }
  });

  it('lets UI text scale to 200 % and clamps display text at 1.3x', () => {
    expect(typeStyle('body.md').maxFontScale).toBe(MAX_FONT_SCALE);
    expect(typeStyle('display.lg').maxFontScale).toBe(1.3);
    for (const group of Object.values(typeScale)) {
      for (const style of Object.values(group)) {
        expect(style.lineHeight).toBeGreaterThanOrEqual(style.size);
      }
    }
  });

  it('builds two-layer shadows that are stronger in dark mode', () => {
    expect(boxShadow(0, 'light')).toBeUndefined();
    expect(boxShadow(1, 'light')).toBe(
      '0px 1px 1px rgba(40, 16, 50, 0.14), 0px 2px 4px rgba(40, 16, 50, 0.08)',
    );
    expect(boxShadow(4, 'dark')).toBe(
      '0px 8px 8px rgba(0, 0, 0, 0.12), 0px 24px 48px rgba(0, 0, 0, 0.352)',
    );
    expect(elevation[3].scale).toBe(1.04);
  });

  it('generates kebab-case CSS variables for web', () => {
    const vars = cssVariables('dark');
    expect(vars['--color-bg-paper-alt']).toBe('#221B2C');
    expect(vars['--color-border-hairline']).toBe('rgba(255, 246, 240, 0.12)');
  });
});
