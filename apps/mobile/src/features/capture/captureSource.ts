/**
 * Capture seam (T-53, US-7). Screens depend on this interface only, so the camera library can change without
 * touching them. Today: the system camera and photo picker through expo-image-picker. The live in-app camera
 * (react-native-vision-camera per ADR-0002, or expo-camera) is pending the capture spike, which needs a device;
 * AC-7.1 (live camera <= 1.5 s) and AC-7.2 (shutter <= 300 ms) are measured once that implementation lands.
 */

/** A photo in app-private storage (the app's cache directory); nothing is copied to the gallery or uploaded. */
export interface CapturedPhoto {
  uri: string;
  width: number;
  height: number;
  origin: 'camera' | 'library';
}

export type CaptureOutcome = { kind: 'photo'; photo: CapturedPhoto } | { kind: 'cancelled' };

export interface CaptureSource {
  /** Takes a photo. Callers check camera permission first (CameraPermissions). */
  takePhoto(): Promise<CaptureOutcome>;
  /** Imports one photo from the library; works without camera access. */
  pickFromLibrary(): Promise<CaptureOutcome>;
  /**
   * Android can destroy the activity while the system camera is open; the photo then arrives on the next start.
   * Returns it once, or null.
   */
  recoverPending(): Promise<CapturedPhoto | null>;
}

/** Fields shared by picker results (expo-image-picker `ImagePickerAsset`). */
export interface PickedAssetLike {
  readonly uri: string;
  readonly width: number;
  readonly height: number;
  readonly type?: string | null;
}

/** Keeps the first image asset; anything else (no asset, a video) counts as cancelled. */
export function toCaptureOutcome(
  result: { canceled: boolean; assets: readonly PickedAssetLike[] | null },
  origin: CapturedPhoto['origin'],
): CaptureOutcome {
  const asset = result.canceled ? undefined : result.assets?.[0];
  if (!asset || (asset.type != null && asset.type !== 'image')) return { kind: 'cancelled' };
  return {
    kind: 'photo',
    photo: { uri: asset.uri, width: asset.width, height: asset.height, origin },
  };
}
