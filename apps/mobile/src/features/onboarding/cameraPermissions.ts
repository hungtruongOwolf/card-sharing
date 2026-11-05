/**
 * Camera permission seam for the onboarding explainer (AC-3.1) and, later, the capture screen.
 * The OS permission is the same whichever library asks for it, so the capture camera
 * (react-native-vision-camera, ADR-0002) can replace the adapter without touching the screens.
 */

export type CameraPermissionStatus =
  /** Never asked: show the explainer with "Allow camera". */
  | 'undetermined'
  /** Denied but the OS will show the prompt again. */
  | 'deniedCanAsk'
  /** Denied for good (or restricted): only Settings can turn it on. */
  | 'blocked'
  | 'granted';

export interface CameraPermissions {
  get(): Promise<CameraPermissionStatus>;
  request(): Promise<CameraPermissionStatus>;
  openSettings(): Promise<void>;
  /** Opens the photo library picker (works without camera access). Resolves true when a photo was chosen. */
  pickFromLibrary(): Promise<boolean>;
}

/** Shape shared by Expo permission responses (expo-modules-core `PermissionResponse`). */
export interface PermissionResponseLike {
  readonly granted: boolean;
  readonly status: string;
  readonly canAskAgain: boolean;
}

export function toCameraPermissionStatus(response: PermissionResponseLike): CameraPermissionStatus {
  if (response.granted) return 'granted';
  if (response.status === 'undetermined') return 'undetermined';
  return response.canAskAgain ? 'deniedCanAsk' : 'blocked';
}
