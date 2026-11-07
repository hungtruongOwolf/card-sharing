import { assetUri } from '../assetUri.web';

const asAsset = (value: unknown) => value as number;

it('returns the hosted path of a font asset', () => {
  expect(assetUri(asAsset('/assets/fonts/anton/Anton-Regular.ttf'))).toBe(
    '/assets/fonts/anton/Anton-Regular.ttf',
  );
});

it('returns the uri of an image asset', () => {
  expect(assetUri(asAsset({ uri: '/assets/icon.png', width: 1, height: 1 }))).toBe(
    '/assets/icon.png',
  );
});

it('returns null for anything else', () => {
  expect(assetUri(asAsset(12))).toBeNull();
  expect(assetUri(asAsset(null))).toBeNull();
});
