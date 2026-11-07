import { isLocalPhotoUri } from '../../platform/localPhoto';
import type { CapturedPhoto } from '../capture';

/**
 * Route params of the editor (`/editor?uri=…&width=…&height=…&origin=…`). Route params can come from a deep link,
 * so they are untrusted: only a photo the app holds is accepted (`file://` in the app's storage; on the web a `blob:`
 * URL of this origin, src/platform/localPhoto.web.ts), never a remote URL that would make the editor fetch from the
 * network (prd-mvp AC-7.5).
 */
export type EditorParams = Record<string, string>;

const MAX_SIDE = 20_000;

export function editorParams(photo: CapturedPhoto): EditorParams {
  return {
    uri: photo.uri,
    width: String(photo.width),
    height: String(photo.height),
    origin: photo.origin,
  };
}

const single = (value: unknown): string | undefined =>
  typeof value === 'string' ? value : undefined;

function side(value: unknown): number | null {
  const parsed = Number(single(value));
  return Number.isSafeInteger(parsed) && parsed > 0 && parsed <= MAX_SIDE ? parsed : null;
}

export function parseEditorPhoto(params: Readonly<Record<string, unknown>>): CapturedPhoto | null {
  const uri = single(params.uri);
  const width = side(params.width);
  const height = side(params.height);
  const origin = single(params.origin);
  if (!uri || !isLocalPhotoUri(uri) || width === null || height === null) return null;
  if (origin !== 'camera' && origin !== 'library') return null;
  return { uri, width, height, origin };
}
