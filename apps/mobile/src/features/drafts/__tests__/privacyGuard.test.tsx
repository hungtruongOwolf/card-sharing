/**
 * Privacy guard (US-9, prd-mvp privacy NFR): drafts and bakes never leave the device. Two checks: no network
 * API is reachable from the drafts and card modules (static), and a full save / list / reopen / autosave run makes
 * no request (dynamic).
 */
import { act, renderHook } from '@testing-library/react-native';

import { registry } from '@card-sharing/effects-registry';

import { ThemeProvider, ToastProvider } from '../../../ui';
import { styleChoice } from '../../editor/editorState';
import { createDraftStore, memoryKeyValueStore } from '../draftStore';
import { AUTOSAVE_DELAY_MS, useDraftAutosave } from '../useDraftAutosave';

// eslint-disable-next-line @typescript-eslint/no-require-imports -- jest.mock factories must use require()
jest.mock('expo-secure-store', () => require('../../../test/secureStoreMock').secureStoreMock);

// Node built-ins for this static check (the app tsconfig has no @types/node; jest runs on Node).
const { readdirSync, readFileSync, statSync } = jest.requireActual<{
  readdirSync(path: string): string[];
  readFileSync(path: string, encoding: 'utf8'): string;
  statSync(path: string): { isDirectory(): boolean };
}>('fs');
const { join } = jest.requireActual<{ join(...parts: string[]): string }>('path');
declare const __dirname: string;

const NETWORK = [
  /\bfetch\s*\(/,
  /XMLHttpRequest/,
  /WebSocket/,
  /@card-sharing\/api-client/,
  /features\/auth/,
  /from '\.\.\/auth'/,
  /friendsApi/,
  /expo-file-system/,
  /uploadAsync/,
];

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return name === '__tests__' ? [] : sources(path);
    return /\.tsx?$/.test(name) ? [path] : [];
  });
}

describe('privacy guard: drafts never upload', () => {
  it.each(['drafts', 'cards'])('features/%s imports no network or upload API', (feature) => {
    const files = sources(join(__dirname, '..', '..', feature));
    expect(files.length).toBeGreaterThan(0);
    for (const file of files) {
      const text = readFileSync(file, 'utf8');
      for (const pattern of NETWORK) {
        if (pattern.test(text)) throw new Error(`${file} matches ${pattern}`);
      }
    }
  });

  it('saves, lists, reopens and auto-saves drafts without a single request', async () => {
    jest.useFakeTimers();
    const fetchSpy = jest.fn(() => Promise.reject(new Error('offline')));
    const original = globalThis.fetch;
    globalThis.fetch = fetchSpy as unknown as typeof fetch;
    try {
      const kv = memoryKeyValueStore();
      const store = createDraftStore(kv);
      const photo = { uri: 'file:///cache/a.jpg', width: 3, height: 4, origin: 'library' as const };
      const draft = {
        cut: styleChoice(registry.get('cut.none')!),
        finish: null,
        effects: [],
        wholePhoto: true,
        caption: 'offline',
      };
      const { result } = await renderHook(() => useDraftAutosave(store, photo), {
        wrapper: ({ children }) => (
          <ThemeProvider scheme="light" reducedMotion={false}>
            <ToastProvider>{children}</ToastProvider>
          </ThemeProvider>
        ),
      });
      await act(async () => {
        result.current.onDraftChange(draft);
        result.current.onDraftChange({ ...draft, caption: 'offline 2' });
        jest.advanceTimersByTime(AUTOSAVE_DELAY_MS);
      });
      jest.useRealTimers();
      const [saved, ...rest] = await store.list();
      expect(rest).toEqual([]);
      expect(saved!.draft.caption).toBe('offline 2');
      expect(await store.get(saved!.id)).toEqual(saved);
      expect(fetchSpy).not.toHaveBeenCalled();
    } finally {
      jest.useRealTimers();
      globalThis.fetch = original;
    }
  });
});
