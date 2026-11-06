import { screen, userEvent, waitFor } from '@testing-library/react-native';

import { registry } from '@card-sharing/effects-registry';

import { t } from '../../../i18n';
import { renderThemed } from '../../../test/render';
import { newLocalCard } from '../../cards/localCard';
import { styleChoice } from '../../editor/editorState';
import { createDraftStore, memoryKeyValueStore } from '../draftStore';
import { DraftsScreen } from '../DraftsScreen';
import { LOW_STORAGE_BYTES } from '../housekeeping';

// eslint-disable-next-line @typescript-eslint/no-require-imports -- jest.mock factories must use require()
jest.mock('expo-secure-store', () => require('../../../test/secureStoreMock').secureStoreMock);

const NOW = new Date('2026-12-10T12:00:00.000Z');
const photo = { uri: 'file:///cache/a.jpg', width: 300, height: 400, origin: 'camera' as const };
const draft = (caption: string) => ({
  cut: styleChoice(registry.get('cut.diecut')!),
  finish: null,
  wholePhoto: false,
  caption,
});

async function setup(storageFree: number | null = null) {
  const store = createDraftStore(memoryKeyValueStore());
  const fresh = newLocalCard(photo, draft('beach day'), new Date('2026-12-09T12:00:00.000Z'), 'a1');
  const old = newLocalCard(photo, draft(''), new Date('2026-10-01T12:00:00.000Z'), 'b2');
  await store.save(fresh);
  await store.save(old);
  const onOpen = jest.fn();
  const onSend = jest.fn();
  await renderThemed(
    <DraftsScreen
      store={store}
      now={() => NOW}
      storageInfo={{ freeBytes: () => Promise.resolve(storageFree) }}
      onOpen={onOpen}
      onSend={onSend}
      onNew={jest.fn()}
    />,
  );
  await screen.findByTestId('drafts');
  return { store, fresh, old, onOpen, onSend };
}

describe('DraftsScreen', () => {
  it('lists drafts newest first and reopens one', async () => {
    const { fresh, onOpen } = await setup();
    expect(screen.getByText('beach day')).toBeTruthy();
    expect(screen.getByText(t('drafts.untitled'))).toBeTruthy();
    expect(screen.getByText(t('drafts.edited', { days: 1 }))).toBeTruthy();
    await userEvent
      .setup()
      .press(screen.getByRole('button', { name: t('drafts.open.a11y', { title: 'beach day' }) }));
    expect(onOpen).toHaveBeenCalledWith(fresh);
  });

  it('sends a draft from the tray', async () => {
    const { fresh, onSend } = await setup();
    await userEvent
      .setup()
      .press(screen.getByRole('button', { name: t('drafts.send.a11y', { title: 'beach day' }) }));
    expect(onSend).toHaveBeenCalledWith(fresh);
  });

  it('offers to clean up drafts older than 30 days, after a confirm', async () => {
    const { store, fresh } = await setup();
    const user = userEvent.setup();
    expect(screen.getByText(t('drafts.cleanup.body', { count: 1, days: 30 }))).toBeTruthy();
    await user.press(screen.getByRole('button', { name: t('drafts.cleanup.action') }));
    await user.press(screen.getByRole('button', { name: t('drafts.confirm.delete') }));
    await waitFor(async () => expect(await store.list()).toEqual([fresh]));
    await waitFor(() => expect(screen.queryByTestId('drafts-cleanup')).toBeNull());
  });

  it('deletes one draft only after a confirm, and Keep cancels', async () => {
    const { store } = await setup();
    const user = userEvent.setup();
    const del = () =>
      screen.getByRole('button', { name: t('drafts.delete.a11y', { title: 'beach day' }) });
    await user.press(del());
    await user.press(screen.getByRole('button', { name: t('drafts.confirm.cancel') }));
    expect(await store.list()).toHaveLength(2);
    await user.press(del());
    await user.press(screen.getByRole('button', { name: t('drafts.confirm.delete') }));
    await waitFor(async () => expect((await store.list()).map((c) => c.id)).toEqual(['b2']));
  });

  it('warns when the phone is almost full', async () => {
    await setup(LOW_STORAGE_BYTES - 1);
    expect(await screen.findByTestId('drafts-low-storage')).toBeTruthy();
  });

  it('stays quiet when free space is unknown', async () => {
    await setup(null);
    expect(screen.queryByTestId('drafts-low-storage')).toBeNull();
  });
});
