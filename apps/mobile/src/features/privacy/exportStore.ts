import { memoryKeyValueStore, secureKeyValueStore, type KeyValueStore } from '../drafts/draftStore';
import type { DataExport, ExportStatus } from './privacyApi';

/**
 * Device memory of the latest data export request (T-155). The API has no "list my exports" route and reading an
 * export's status needs a fresh re-auth, so the app remembers the id and the times the server returned when it
 * queued the request, to show the 48-hour wait without asking the user to confirm it's them. Never holds a download
 * path (single use, issued per read) or a token. Forgotten on sign-out with the other account data.
 */
export interface RememberedExport {
  readonly id: string;
  readonly status: ExportStatus;
  /** Epoch milliseconds. */
  readonly requestedAt: number;
  /** Epoch milliseconds. */
  readonly availableAt: number;
}

export interface ExportStore {
  load(): Promise<RememberedExport | null>;
  save(item: DataExport | RememberedExport): Promise<void>;
  clear(): Promise<void>;
}

/** SecureStore keys allow only alphanumerics, `.`, `-` and `_`. Bump the version on a format change. */
export const EXPORT_KEY = 'cs.privacy.export.v1';

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const STATUSES: readonly ExportStatus[] = [
  'queued',
  'building',
  'ready',
  'downloaded',
  'expired',
  'cancelled',
  'failed',
];

function parse(raw: string | null): RememberedExport | null {
  if (raw === null) return null;
  try {
    const v = JSON.parse(raw) as Partial<Record<keyof RememberedExport, unknown>>;
    const status = STATUSES.find((s) => s === v.status);
    if (typeof v.id !== 'string' || !UUID.test(v.id) || !status) return null;
    if (typeof v.requestedAt !== 'number' || !Number.isFinite(v.requestedAt)) return null;
    if (typeof v.availableAt !== 'number' || !Number.isFinite(v.availableAt)) return null;
    return { id: v.id, status, requestedAt: v.requestedAt, availableAt: v.availableAt };
  } catch {
    return null;
  }
}

export function createExportStore(kv: KeyValueStore = secureKeyValueStore()): ExportStore {
  return {
    load: async () => parse(await kv.getItem(EXPORT_KEY).catch(() => null)),
    save: ({ id, status, requestedAt, availableAt }) =>
      kv.setItem(EXPORT_KEY, JSON.stringify({ id, status, requestedAt, availableAt })),
    clear: () => kv.deleteItem(EXPORT_KEY),
  };
}

/** In memory, for tests and previews. */
export const memoryExportStore = (): ExportStore => createExportStore(memoryKeyValueStore());

let device: ExportStore | null = null;

/** The app's one export store (shared by the screen and the sign-out cleanup). */
export function deviceExportStore(): ExportStore {
  device ??= createExportStore();
  return device;
}
