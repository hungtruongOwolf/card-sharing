import { isLocalPhotoUri } from '../localPhoto.web';

const ORIGIN = 'http://localhost:8081';

it('native accepts only app storage file URIs', () => {
  // Jest resolves the native file (the import resolver used by lint would pick the .web file).
  const native: { isLocalPhotoUri: (uri: string) => boolean } = jest.requireActual('../localPhoto');
  expect(native.isLocalPhotoUri('file:///data/user/0/app/cache/photo.jpg')).toBe(true);
  expect(native.isLocalPhotoUri(`blob:${ORIGIN}/2f1c-uuid`)).toBe(false);
  expect(native.isLocalPhotoUri('https://example.com/photo.jpg')).toBe(false);
});

it('web accepts only blob URLs this origin created', () => {
  expect(isLocalPhotoUri(`blob:${ORIGIN}/2f1c-uuid`, ORIGIN)).toBe(true);
  expect(isLocalPhotoUri('blob:https://evil.example/2f1c-uuid', ORIGIN)).toBe(false);
  expect(isLocalPhotoUri('https://example.com/photo.jpg', ORIGIN)).toBe(false);
  expect(isLocalPhotoUri('data:image/png;base64,AAAA', ORIGIN)).toBe(false);
  expect(isLocalPhotoUri(`blob:${ORIGIN}/2f1c-uuid`, 'null')).toBe(false);
  expect(isLocalPhotoUri('file:///photo.jpg', ORIGIN)).toBe(false);
});
