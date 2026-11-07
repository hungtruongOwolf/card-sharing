import { useEffect } from 'react';
import { Platform } from 'react-native';
import {
  SensorType,
  useAnimatedReaction,
  useAnimatedSensor,
  useSharedValue,
  type SharedValue,
} from 'react-native-reanimated';

import { gravityAxisSign, INITIAL_TILT_STATE, stepTilt, tiltChanged, type Tilt } from './gyroTilt';

const AXIS_SIGN = gravityAxisSign(Platform.OS);

export interface GyroTiltDriverProps {
  /** Written on the UI thread whenever the tilt moves more than the noise threshold. */
  tilt: SharedValue<Tilt>;
  /** Reports once whether the device has a gravity sensor. */
  onAvailability?: (available: boolean) => void;
}

/**
 * Feeds the gravity sensor into `tilt` entirely on the UI thread (no JS per sample). Mount it only
 * while gyro tilt is wanted: unmounting unregisters the sensor (reduced motion never mounts it).
 * One driver serves every card on screen.
 */
export function GyroTiltDriver({ tilt, onAvailability }: GyroTiltDriverProps) {
  const { sensor, isAvailable } = useAnimatedSensor(SensorType.GRAVITY, { interval: 'auto' });
  const state = useSharedValue(INITIAL_TILT_STATE);

  useEffect(() => {
    onAvailability?.(isAvailable);
  }, [isAvailable, onAvailability]);

  useEffect(
    () => () => {
      tilt.set({ x: 0, y: 0 });
    },
    [tilt],
  );

  useAnimatedReaction(
    () => sensor.get(),
    (g) => {
      const next = stepTilt(state.get(), g.x * AXIS_SIGN, g.y * AXIS_SIGN, g.z * AXIS_SIGN);
      state.set(next);
      if (tiltChanged(next, tilt.get())) tilt.set({ x: next.x, y: next.y });
    },
  );

  return null;
}
