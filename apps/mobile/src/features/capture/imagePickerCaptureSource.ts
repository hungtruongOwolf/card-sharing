import * as ImagePicker from 'expo-image-picker';

import { toCaptureOutcome, type CaptureSource } from './captureSource';

/**
 * expo-image-picker implementation. Results land in the app's cache directory (app-private). Images only
 * (the MVP has no video) and no EXIF, so location metadata is never read.
 */
const OPTIONS: ImagePicker.ImagePickerOptions = { mediaTypes: ['images'], exif: false, quality: 1 };

export function createImagePickerCaptureSource(): CaptureSource {
  return {
    takePhoto: async () => toCaptureOutcome(await ImagePicker.launchCameraAsync(OPTIONS), 'camera'),
    pickFromLibrary: async () =>
      toCaptureOutcome(await ImagePicker.launchImageLibraryAsync(OPTIONS), 'library'),
    recoverPending: async () => {
      const pending = await ImagePicker.getPendingResultAsync();
      if (!pending || 'code' in pending) return null;
      const outcome = toCaptureOutcome(pending, 'camera');
      return outcome.kind === 'photo' ? outcome.photo : null;
    },
  };
}
