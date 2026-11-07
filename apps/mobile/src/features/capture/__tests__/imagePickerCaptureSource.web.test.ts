import * as ImagePicker from 'expo-image-picker';

import { createImagePickerCaptureSource } from '../imagePickerCaptureSource.web';

jest.mock('expo-image-picker', () => ({
  launchCameraAsync: jest.fn(),
  launchImageLibraryAsync: jest.fn(),
  getPendingResultAsync: jest.fn(() => {
    throw new Error('not available on web');
  }),
}));

const picked = {
  canceled: false,
  assets: [{ uri: 'blob:http://localhost:8081/abc', width: 800, height: 600, type: 'image' }],
};

it('maps the file input result to a captured photo', async () => {
  jest.mocked(ImagePicker.launchCameraAsync).mockResolvedValue(picked as never);
  await expect(createImagePickerCaptureSource().takePhoto()).resolves.toEqual({
    kind: 'photo',
    photo: { uri: 'blob:http://localhost:8081/abc', width: 800, height: 600, origin: 'camera' },
  });
});

it('has no pending result to recover and never calls the native-only API', async () => {
  await expect(createImagePickerCaptureSource().recoverPending()).resolves.toBeNull();
  expect(ImagePicker.getPendingResultAsync).not.toHaveBeenCalled();
});
