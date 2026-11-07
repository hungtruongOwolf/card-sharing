import * as ImagePicker from 'expo-image-picker';

import { toCaptureOutcome, type CaptureSource } from './captureSource';

/**
 * Web capture (ADR-0009): expo-image-picker's web implementation opens a file input (`capture` set for the camera,
 * so a phone browser offers its camera). The photo is a `blob:` URL of the chosen file: it stays in the browser and
 * lives as long as the tab; drafts keep its bytes in IndexedDB so a reload restores it
 * (features/drafts/draftPhotos.web.ts). There is no pending result to recover (that is an Android activity concern).
 */
const OPTIONS: ImagePicker.ImagePickerOptions = { mediaTypes: ['images'], exif: false, quality: 1 };

export function createImagePickerCaptureSource(): CaptureSource {
  return {
    takePhoto: async () => toCaptureOutcome(await ImagePicker.launchCameraAsync(OPTIONS), 'camera'),
    pickFromLibrary: async () =>
      toCaptureOutcome(await ImagePicker.launchImageLibraryAsync(OPTIONS), 'library'),
    recoverPending: async () => null,
  };
}
