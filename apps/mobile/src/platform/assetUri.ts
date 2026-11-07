import { Image } from 'react-native';

/**
 * The URI of a bundled asset (`require('./file.ttf')`). Native: Metro registers the asset and React Native
 * resolves it. The web target resolves `assetUri.web.ts`.
 */
export function assetUri(asset: number): string | null {
  return Image.resolveAssetSource(asset)?.uri ?? null;
}
