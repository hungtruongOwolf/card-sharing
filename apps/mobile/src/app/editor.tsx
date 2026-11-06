import Constants from 'expo-constants';
import { Redirect, useLocalSearchParams, useRouter } from 'expo-router';
import { useEffect, useMemo, useState } from 'react';

import type { LocalCard } from '../features/cards';
import type { CapturedPhoto } from '../features/capture';
import { deviceDraftStore, useDraftAutosave } from '../features/drafts';
import {
  CardEditorScreen,
  createSyntheticMaskSource,
  noMaskSource,
  parseEditorPhoto,
} from '../features/editor';
import { useRemoteConfig } from '../features/remote-config';
import { t } from '../i18n';
import { Screen, Text } from '../ui';

/**
 * Card editor (T-64). A new photo arrives as route params from capture; `?draft=<id>` reopens a draft from the
 * drafts tray (T-68). Every change auto-saves to the drafts tray. Until the real segmenters land (T-54/T-55),
 * development builds use a synthetic subject mask and release builds make whole-photo cards.
 */
export default function EditorRoute() {
  const params = useLocalSearchParams();
  const draftId = typeof params.draft === 'string' ? params.draft : null;
  const photo = useMemo(() => (draftId ? null : parseEditorPhoto(params)), [draftId, params]);
  const [draft, setDraft] = useState<LocalCard | null | 'loading'>(draftId ? 'loading' : null);

  useEffect(() => {
    if (!draftId) return;
    let cancelled = false;
    deviceDraftStore()
      .get(draftId)
      .then(
        (card) => !cancelled && setDraft(card),
        () => !cancelled && setDraft(null),
      );
    return () => {
      cancelled = true;
    };
  }, [draftId]);

  if (draftId) {
    if (draft === 'loading') {
      return (
        <Screen centered>
          <Text tone="muted">{t('editor.loading')}</Text>
        </Screen>
      );
    }
    if (!draft) return <Redirect href="/drafts" />;
    return <Editor photo={draft.photo} existing={draft} />;
  }
  if (!photo) return <Redirect href="/capture" />;
  return <Editor photo={photo} existing={null} />;
}

function Editor({ photo, existing }: { photo: CapturedPhoto; existing: LocalCard | null }) {
  const router = useRouter();
  const maskSource = useMemo(() => (__DEV__ ? createSyntheticMaskSource() : noMaskSource), []);
  const remoteConfig = useRemoteConfig();
  const onDraftChange = useDraftAutosave(deviceDraftStore(), photo, existing);
  return (
    <CardEditorScreen
      photo={photo}
      maskSource={maskSource}
      appVersion={Constants.expoConfig?.version ?? '0.0.0'}
      remoteConfig={remoteConfig?.registry}
      captionMax={remoteConfig?.captionMaxGraphemes}
      textFilter={remoteConfig?.textFilter}
      initialDraft={existing?.draft}
      onDraftChange={onDraftChange}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/capture'))}
    />
  );
}
