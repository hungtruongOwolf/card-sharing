import { Canvas, Path } from '@shopify/react-native-skia';
import { useMemo, type ReactNode } from 'react';
import { StyleSheet, View } from 'react-native';

import { palette, radius } from '@card-sharing/tokens';

import { encodeQr, qrPath, type ErrorCorrection } from './qrEncoder';

/** Modules of light margin around the code, as the spec asks for reliable scanning. */
const QUIET_ZONE = 4;

export interface QrCodeProps {
  value: string;
  /** Rendered width and height in points. */
  size: number;
  accessibilityLabel: string;
  level?: ErrorCorrection;
  /** Shown instead when `value` does not fit a QR code (never expected for an invite link). */
  fallback?: ReactNode;
}

/**
 * A QR code drawn by Skia as one path. Always warm ink on white whatever the theme: inverted (light on dark)
 * codes are not read by every scanner. The image is one accessibility element with the caller's label.
 */
export function QrCode({
  value,
  size,
  accessibilityLabel,
  level = 'M',
  fallback = null,
}: QrCodeProps) {
  const matrix = useMemo(() => encodeQr(value, level), [value, level]);
  const path = useMemo(() => (matrix ? qrPath(matrix, QUIET_ZONE) : ''), [matrix]);
  if (!matrix) return <>{fallback}</>;
  const scale = size / (matrix.size + QUIET_ZONE * 2);
  return (
    <View
      testID="qr-code"
      accessible
      accessibilityRole="image"
      accessibilityLabel={accessibilityLabel}
      style={[styles.plate, { width: size, height: size }]}
    >
      <Canvas style={{ width: size, height: size }}>
        <Path path={path} color={palette.ink.light} antiAlias={false} transform={[{ scale }]} />
      </Canvas>
    </View>
  );
}

const styles = StyleSheet.create({
  plate: { backgroundColor: palette.white, borderRadius: radius.md, overflow: 'hidden' },
});
