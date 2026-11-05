import * as ImagePicker from 'expo-image-picker';
import { Linking } from 'react-native';

import { toCameraPermissionStatus, type CameraPermissions } from './cameraPermissions';

/**
 * expo-image-picker adapter: it is in Expo Go and is needed anyway for library import (AC-3.1), which the
 * capture camera does not provide. Settings opens through React Native's `Linking.openSettings()`.
 */
export function createExpoCameraPermissions(): CameraPermissions {
  return {
    get: async () => toCameraPermissionStatus(await ImagePicker.getCameraPermissionsAsync()),
    request: async () =>
      toCameraPermissionStatus(await ImagePicker.requestCameraPermissionsAsync()),
    openSettings: () => Linking.openSettings(),
    pickFromLibrary: async () => {
      // Images only: the MVP has no video (prd-mvp), and iOS asks for library access to export videos.
      const result = await ImagePicker.launchImageLibraryAsync({ mediaTypes: ['images'] });
      return !result.canceled && (result.assets?.length ?? 0) > 0;
    },
  };
}
