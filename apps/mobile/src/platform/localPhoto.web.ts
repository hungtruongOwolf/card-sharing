/**
 * Web (ADR-0009): a captured photo is a `blob:` URL that this page created for the chosen file
 * (`blob:<this origin>/<uuid>`). Such a URL resolves only inside this origin, so accepting it can never make the
 * editor fetch from the network; `http(s):`, `data:` and other origins' blobs are refused.
 */
export function isLocalPhotoUri(
  uri: string,
  origin: string = globalThis.location?.origin ?? '',
): boolean {
  return origin !== '' && origin !== 'null' && uri.startsWith(`blob:${origin}/`);
}
