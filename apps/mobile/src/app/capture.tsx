import { useMemo } from 'react';

import { CaptureScreen, createImagePickerCaptureSource } from '../features/capture';
import { createExpoCameraPermissions } from '../features/onboarding';

/** Capture or import a photo (T-53). The cutout step (T-54+) will receive the photo through `onUse`. */
export default function CaptureRoute() {
  const source = useMemo(() => createImagePickerCaptureSource(), []);
  const permissions = useMemo(() => createExpoCameraPermissions(), []);
  return <CaptureScreen source={source} permissions={permissions} />;
}
