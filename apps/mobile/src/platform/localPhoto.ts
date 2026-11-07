/**
 * Whether `uri` points at a photo the app itself holds, never at the network (prd-mvp AC-7.5). Native: a `file://`
 * URI in the app's storage. The web target resolves `localPhoto.web.ts`.
 */
export function isLocalPhotoUri(uri: string): boolean {
  return uri.startsWith('file://');
}
