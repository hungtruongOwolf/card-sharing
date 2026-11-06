import { Skia, type SkImage } from '@shopify/react-native-skia';
import type { TextureLod } from '@card-sharing/canvas-engine';

import { downscalePhoto } from '../editor/cardScene';
import { createMemoryBoardRepository, type BoardRepository } from './boardRepository';
import { createTextureCache, type TextureCache } from './textureCache';

/** Downloads a signed card render and scales it to the texture level (the full-size decode is dropped). */
export async function loadRemoteTexture(url: string, lod: TextureLod): Promise<SkImage | null> {
  const data = await Skia.Data.fromURI(url);
  const image = Skia.Image.MakeImageFromEncoded(data);
  return image ? downscalePhoto(image, lod) : null;
}

let textures: TextureCache<SkImage> | null = null;
let repository: BoardRepository | null = null;

/** One texture cache for every board screen, so reopening a board reuses decoded pins. */
export function appBoardTextures(): TextureCache<SkImage> {
  textures ??= createTextureCache({ load: loadRemoteTexture });
  return textures;
}

/**
 * The app's boards until the board endpoints and offline sync land (T-101, T-105): in memory, starting empty.
 */
export function appBoardRepository(): BoardRepository {
  repository ??= createMemoryBoardRepository();
  return repository;
}
