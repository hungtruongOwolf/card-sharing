import { Skia, type SkImage } from '@shopify/react-native-skia';
import { useMemo } from 'react';

import type { TextureLod } from '@card-sharing/canvas-engine';

import { useAuthClient, type AuthClient } from '../auth';
import { downscalePhoto } from '../editor/cardScene';
import { createBoardMembersApi, type BoardMembersApi } from './boardMembersApi';
import { createBoardsApi } from './boardsApi';
import { createBoardStore, type BoardStore } from './boardStore';
import { createSyncedBoardRepository, type SyncedBoardRepository } from './boardSync';
import { createPinDisclosure, type PinDisclosure } from './pinDisclosure';
import { createTextureCache, type TextureCache } from './textureCache';

/** Downloads a signed card render and scales it to the texture level (the full-size decode is dropped). */
export async function loadRemoteTexture(url: string, lod: TextureLod): Promise<SkImage | null> {
  const data = await Skia.Data.fromURI(url);
  const image = Skia.Image.MakeImageFromEncoded(data);
  return image ? downscalePhoto(image, lod) : null;
}

let textures: TextureCache<SkImage> | null = null;
let store: BoardStore | null = null;
let repository: { client: AuthClient; repo: SyncedBoardRepository } | null = null;

/** One texture cache for every board screen, so reopening a board reuses decoded pins. */
export function appBoardTextures(): TextureCache<SkImage> {
  textures ??= createTextureCache({ load: loadRemoteTexture });
  return textures;
}

/** The device's offline board copies (SecureStore-backed), created on first use. */
export function deviceBoardStore(): BoardStore {
  store ??= createBoardStore();
  return store;
}

/** The app's boards over the signed-in auth client: local first, synced with the server (T-105). */
export function appBoardRepository(client: AuthClient): SyncedBoardRepository {
  if (repository?.client !== client) {
    repository = {
      client,
      repo: createSyncedBoardRepository({
        api: createBoardsApi(client.fetch),
        store: deviceBoardStore(),
      }),
    };
  }
  return repository.repo;
}

let disclosure: PinDisclosure | null = null;

/** The device's record of the one-time shared-board explanation (AC-19.5). */
export function devicePinDisclosure(): PinDisclosure {
  disclosure ??= createPinDisclosure();
  return disclosure;
}

/** Members and invitations over the signed-in auth client (T-108). */
export function useBoardMembersApi(): BoardMembersApi {
  const client = useAuthClient();
  return useMemo(() => createBoardMembersApi(client.fetch), [client]);
}

export function useBoardRepository(): SyncedBoardRepository {
  const client = useAuthClient();
  return useMemo(() => appBoardRepository(client), [client]);
}

/**
 * Sign-out: board copies hold friends' cards and names, so they never outlive the session. Drops the in-memory
 * repository too, so the next account starts empty.
 */
export async function clearDeviceBoards(): Promise<void> {
  repository = null;
  await deviceBoardStore().clear();
  await devicePinDisclosure().clear();
}
