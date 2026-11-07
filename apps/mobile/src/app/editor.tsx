import Constants from 'expo-constants';
import { Redirect, useLocalSearchParams, useRouter } from 'expo-router';
import { useCallback, useEffect, useMemo, useState } from 'react';

import { killedIds, registry } from '@card-sharing/effects-registry';

import { useAiEffects } from '../features/ai-effects';
import { newLocalCard, prepareForSend, type LocalCard } from '../features/cards';
import type { CapturedPhoto } from '../features/capture';
import { deviceDraftStore, useDraftAutosave } from '../features/drafts';
import {
  appMaskSource,
  CardEditorScreen,
  parseEditorPhoto,
  type CardDraft,
} from '../features/editor';
import { useRemoteConfig } from '../features/remote-config';
import { parseReplyTarget, replyParams, type ReplyTarget } from '../features/sharing';
import { t } from '../i18n';
import { Screen, Text, useToast } from '../ui';

/**
 * Card editor (T-64). A new photo arrives as route params from capture; `?draft=<id>` reopens a draft from the
 * drafts tray (T-68). Every change auto-saves to the drafts tray. Subject masks come from `appMaskSource()` (web:
 * BiRefNet_lite; native until T-54/T-55: synthetic in development, whole-photo cards in release). Next saves the
 * draft and opens the send screen (T-77). AI effects (Tier 2) bake the same send render as their input.
 */
export default function EditorRoute() {
  const params = useLocalSearchParams();
  const draftId = typeof params.draft === 'string' ? params.draft : null;
  const photo = useMemo(() => (draftId ? null : parseEditorPhoto(params)), [draftId, params]);
  const reply = useMemo(() => parseReplyTarget(params), [params]);
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
    return <Editor photo={draft.photo} existing={draft} reply={reply} />;
  }
  if (!photo) return <Redirect href="/capture" />;
  return <Editor photo={photo} existing={null} reply={reply} />;
}

function Editor({
  photo,
  existing,
  reply,
}: {
  photo: CapturedPhoto;
  existing: LocalCard | null;
  /** ADR-0010: the card this one answers (from the story viewer), forwarded to the send screen. */
  reply: ReplyTarget | null;
}) {
  const router = useRouter();
  const maskSource = appMaskSource();
  const remoteConfig = useRemoteConfig();
  const toast = useToast();
  const { onDraftChange, saveNow } = useDraftAutosave(deviceDraftStore(), photo, existing);
  const killed = useMemo(() => killedIds(remoteConfig?.registry), [remoteConfig]);
  const renderForAi = useCallback(
    async (draft: CardDraft) =>
      (await prepareForSend(newLocalCard(photo, draft), false, { registry, maskSource, killed }))
        .render,
    [photo, maskSource, killed],
  );
  const aiEffects = useAiEffects({ render: renderForAi });
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
      aiEffects={aiEffects}
      onNext={(draft) =>
        saveNow(draft).then(
          (card) =>
            router.push({ pathname: '/send', params: { draft: card.id, ...replyParams(reply) } }),
          () => toast.show({ message: t('drafts.saveFailed'), variant: 'error' }),
        )
      }
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/capture'))}
    />
  );
}
