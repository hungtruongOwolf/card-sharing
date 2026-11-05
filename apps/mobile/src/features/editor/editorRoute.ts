import type { CapturedPhoto } from '../capture';

/**
 * Route params of the editor (`/editor?uri=…&width=…&height=…&origin=…`). Route params can come from a deep link,
 * so they are untrusted: only a `file://` URI (the app's own storage) is accepted, never a remote URL that would
 * make the editor fetch from the network (prd-mvp AC-7.5).
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
  if (!uri || !uri.startsWith('file://') || width === null || height === null) return null;
  if (origin !== 'camera' && origin !== 'library') return null;
  return { uri, width, height, origin };
}
