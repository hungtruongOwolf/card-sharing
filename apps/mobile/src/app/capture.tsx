import { useLocalSearchParams, useRouter } from 'expo-router';
import { useMemo } from 'react';

import { CaptureScreen, createImagePickerCaptureSource } from '../features/capture';
import { editorParams } from '../features/editor';
import { createExpoCameraPermissions } from '../features/onboarding';
import { parseReplyTarget, replyParams } from '../features/sharing';

/** Capture or import a photo (T-53), then style it in the card editor (T-64). */
export default function CaptureRoute() {
  const router = useRouter();
  // ADR-0010: a reply started in the story viewer travels on to the editor and the send screen.
  const reply = parseReplyTarget(useLocalSearchParams());
  const source = useMemo(() => createImagePickerCaptureSource(), []);
  const permissions = useMemo(() => createExpoCameraPermissions(), []);
  return (
    <CaptureScreen
      source={source}
      permissions={permissions}
      onUse={(photo) =>
        router.push({
          pathname: '/editor',
          params: { ...editorParams(photo), ...replyParams(reply) },
        })
      }
    />
  );
}
