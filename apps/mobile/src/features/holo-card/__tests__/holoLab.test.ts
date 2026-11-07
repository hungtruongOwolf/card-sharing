import {
  DEFAULT_TILT_PARAMS,
  gravityAxisSign,
  INITIAL_TILT_STATE,
  stepTilt,
  tiltChanged,
  type TiltState,
} from '../gyroTilt';
import { cardSize, DEFAULT_HOLO_LAB_SETTINGS, liveFillPixels, planHoloModes } from '../holoLab';

const G = 9.81;

/** Gravity of a phone held upright and rolled by `deg` about its long axis (y up the screen). */
const rolled = (deg: number): [number, number, number] => {
  const r = (deg * Math.PI) / 180;
  return [G * Math.sin(r), G * 0.7, G * Math.cos(r) * 0.7];
};

const feed = (state: TiltState, sample: [number, number, number], times: number) => {
  let s = state;
  for (let i = 0; i < times; i += 1) s = stepTilt(s, ...sample);
  return s;
};

describe('stepTilt', () => {
  it('primes the neutral pose on the first sample with zero tilt', () => {
    const s = stepTilt(INITIAL_TILT_STATE, ...rolled(0));
    expect(s.primed).toBe(true);
    expect(s.x).toBe(0);
    expect(s.y).toBe(0);
    expect(Math.hypot(s.nx, s.ny, s.nz)).toBeCloseTo(1, 9);
  });

  it('ignores a zero vector (sensor not ready)', () => {
    expect(stepTilt(INITIAL_TILT_STATE, 0, 0, 0)).toBe(INITIAL_TILT_STATE);
  });

  it('tilts towards a change in pose, then eases back to rest as the neutral catches up', () => {
    const settled = feed(INITIAL_TILT_STATE, rolled(0), 10);
    const moved = feed(settled, rolled(15), 8);
    expect(moved.x).toBeGreaterThan(0.3);
    const rested = feed(moved, rolled(15), 600);
    expect(Math.abs(rested.x)).toBeLessThan(0.02);
  });

  it('mirrors the direction of the roll', () => {
    const settled = feed(INITIAL_TILT_STATE, rolled(0), 10);
    expect(feed(settled, rolled(-15), 8).x).toBeLessThan(-0.3);
  });

  it('clamps to the -1..1 uniform range', () => {
    const settled = feed(INITIAL_TILT_STATE, rolled(0), 10);
    const extreme = feed(settled, rolled(85), 40);
    expect(extreme.x).toBeLessThanOrEqual(1);
    expect(extreme.x).toBeGreaterThan(0.9);
  });

  it('only uses the direction of gravity, not its unit', () => {
    const inG = feed(INITIAL_TILT_STATE, [0, 0.7, 0.7], 5);
    const inMs2 = feed(INITIAL_TILT_STATE, [0, 0.7 * G, 0.7 * G], 5);
    const tiltedG = stepTilt(inG, 0.3, 0.7, 0.6, DEFAULT_TILT_PARAMS);
    const tiltedMs2 = stepTilt(inMs2, 0.3 * G, 0.7 * G, 0.6 * G, DEFAULT_TILT_PARAMS);
    expect(tiltedG.x).toBeCloseTo(tiltedMs2.x, 9);
    expect(tiltedG.y).toBeCloseTo(tiltedMs2.y, 9);
  });
});

describe('gravityAxisSign', () => {
  it('flips iOS and the web (Reanimated normalises browser sensors to the iOS convention), not Android', () => {
    expect(gravityAxisSign('ios')).toBe(-1);
    expect(gravityAxisSign('web')).toBe(-1);
    expect(gravityAxisSign('android')).toBe(1);
  });
});

describe('tiltChanged', () => {
  it('ignores sensor noise below the threshold', () => {
    expect(tiltChanged({ x: 0.1, y: 0.1 }, { x: 0.1015, y: 0.0995 })).toBe(false);
    expect(tiltChanged({ x: 0.1, y: 0.1 }, { x: 0.11, y: 0.1 })).toBe(true);
  });
});

describe('planHoloModes', () => {
  it('makes the first maxLive cards live and bakes the rest', () => {
    expect(planHoloModes(3, 3, false)).toEqual(['live', 'live', 'live']);
    expect(planHoloModes(6, 2, false)).toEqual([
      'live',
      'live',
      'baked',
      'baked',
      'baked',
      'baked',
    ]);
    expect(planHoloModes(3, 0, false)).toEqual(['baked', 'baked', 'baked']);
    expect(planHoloModes(2, 5, false)).toEqual(['live', 'live']);
  });

  it('renders every card as a static sheen under reduced motion', () => {
    expect(planHoloModes(3, 3, true)).toEqual(['static', 'static', 'static']);
  });

  it('defaults to the spike scene: 3 cards, all live', () => {
    const { cardCount, maxLive } = DEFAULT_HOLO_LAB_SETTINGS;
    expect(planHoloModes(cardCount, maxLive, false)).toEqual(['live', 'live', 'live']);
  });
});

describe('cardSize and liveFillPixels', () => {
  it('splits the width into columns at the card aspect', () => {
    expect(cardSize(358, 2, 12, 1.4)).toEqual({ width: 173, height: 242 });
    expect(cardSize(358, 1, 12, 1.4)).toEqual({ width: 358, height: 501 });
  });

  it('counts only live cards and scales with resolutionScale squared', () => {
    const size = { width: 173, height: 242 };
    const modes = planHoloModes(3, 2, false);
    const full = liveFillPixels(modes, size, 2.625, 1);
    expect(full).toBe(2 * Math.round(173 * 2.625) * Math.round(242 * 2.625));
    const half = liveFillPixels(modes, size, 2.625, 0.5);
    expect(half / full).toBeCloseTo(0.25, 2);
    expect(liveFillPixels(planHoloModes(3, 3, true), size, 2.625, 1)).toBe(0);
  });
});
