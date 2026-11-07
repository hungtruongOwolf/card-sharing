/**
 * Maps the gravity vector (Reanimated `useAnimatedSensor(SensorType.GRAVITY)`) to the holo `u_tilt`
 * uniform, -1..1 per axis. Runs on the UI thread for every sensor sample; pure, so it is unit-tested.
 *
 * Tilt is measured against a slowly drifting "neutral" pose, not against lying flat: people hold the
 * phone at any angle, and the foil should react to *changes* in how they hold it. The neutral pose
 * follows the current one with a small per-sample factor, so after the user settles, the tilt eases
 * back to 0 and the shimmer comes to rest.
 */

/**
 * Sign that brings Reanimated's gravity vector to one convention. Core Motion reports gravity pointing down
 * (face-up flat = z -1 g), Android's TYPE_GRAVITY pointing up (z +9.81), so iOS axes are flipped. On the web,
 * Reanimated's backend already flips the Android browser's Generic Sensor readings to the iOS convention
 * (react-native-reanimated 4.7 `JSReanimated.getSensorCallback`), so the web takes the iOS sign, not the Android one.
 * Assumption to verify on a device (T-52 measurement protocol): tilting the right edge down should move the foil
 * the same way everywhere.
 */
export function gravityAxisSign(os: string): 1 | -1 {
  return os === 'android' ? 1 : -1;
}

export interface Tilt {
  x: number;
  y: number;
}

export interface TiltState extends Tilt {
  /** Neutral pose (unit gravity vector); `primed` is false until the first sample. */
  nx: number;
  ny: number;
  nz: number;
  primed: boolean;
}

export interface TiltParams {
  /** Tilt units per unit of gravity-direction change; 2.5 means ~23 degrees reaches full tilt. */
  gain: number;
  /** Share of the gap the neutral pose closes per sample (~1.2 s time constant at 60 Hz for 0.014). */
  neutralFollow: number;
  /** Low-pass factor on the output per sample: 1 = raw sensor, smaller = smoother but laggier. */
  smoothing: number;
}

export const DEFAULT_TILT_PARAMS: TiltParams = { gain: 2.5, neutralFollow: 0.014, smoothing: 0.35 };

/** Output changes smaller than this do not count as motion (see {@link tiltChanged}). */
export const TILT_EPSILON = 0.002;

export const INITIAL_TILT_STATE: TiltState = { x: 0, y: 0, nx: 0, ny: 0, nz: 1, primed: false };

const clamp1 = (v: number) => {
  'worklet';
  return Math.max(-1, Math.min(1, v));
};

/**
 * Advances the tilt by one gravity sample (any unit; only the direction is used).
 * Returns the previous state unchanged for a zero vector (sensor not ready).
 */
export function stepTilt(
  state: TiltState,
  gx: number,
  gy: number,
  gz: number,
  params: TiltParams = DEFAULT_TILT_PARAMS,
): TiltState {
  'worklet';
  const len = Math.sqrt(gx * gx + gy * gy + gz * gz);
  if (!(len > 1e-6)) return state;
  const ux = gx / len;
  const uy = gy / len;
  const uz = gz / len;

  let nx = ux;
  let ny = uy;
  let nz = uz;
  if (state.primed) {
    nx = state.nx + (ux - state.nx) * params.neutralFollow;
    ny = state.ny + (uy - state.ny) * params.neutralFollow;
    nz = state.nz + (uz - state.nz) * params.neutralFollow;
    const nlen = Math.sqrt(nx * nx + ny * ny + nz * nz) || 1;
    nx /= nlen;
    ny /= nlen;
    nz /= nlen;
  }

  const targetX = clamp1((ux - nx) * params.gain);
  const targetY = clamp1((uy - ny) * params.gain);
  return {
    x: state.x + (targetX - state.x) * params.smoothing,
    y: state.y + (targetY - state.y) * params.smoothing,
    nx,
    ny,
    nz,
    primed: true,
  };
}

/** True when the tilt moved enough to be worth a redraw (avoids repainting on sensor noise). */
export function tiltChanged(a: Tilt, b: Tilt, epsilon = TILT_EPSILON): boolean {
  'worklet';
  return Math.abs(a.x - b.x) > epsilon || Math.abs(a.y - b.y) > epsilon;
}
