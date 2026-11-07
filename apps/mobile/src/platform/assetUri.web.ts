/**
 * Web: Expo's Metro asset transformer turns `require('./file.ttf')` into the hosted path itself (a string), or into
 * `{ uri, width, height }` for images, instead of a registry id (@expo/metro-config asset-transformer).
 */
export function assetUri(asset: number): string | null {
  const value: unknown = asset;
  if (typeof value === 'string') return value;
  if (value && typeof value === 'object' && 'uri' in value && typeof value.uri === 'string') {
    return value.uri;
  }
  return null;
}
