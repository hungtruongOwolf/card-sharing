import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { ActivityIndicator, StyleSheet, View } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import { Button, Screen, Text, useTheme, useToast } from '../../ui';
import type { SignInProvider } from '../onboarding/signInProviders';
import type { ExportStore, RememberedExport } from './exportStore';
import { PrivacyError, type DataExport, type ExportStatus, type PrivacyApi } from './privacyApi';
import { formatDateTime, privacyErrorMessage, sessionEnded } from './privacyMessages';
import { useReauth } from './useReauth';

export const EXPORT_QUERY_KEY = ['data-export'] as const;

/** Statuses after which a new export may be requested (subject to the server's 24-hour cooldown). */
const FINISHED: Partial<Record<ExportStatus, MessageKey>> = {
  downloaded: 'dataExport.finished.downloaded',
  expired: 'dataExport.finished.expired',
  cancelled: 'dataExport.finished.cancelled',
  failed: 'dataExport.finished.failed',
};
const HOUR_SECONDS = 3600;

export interface DataExportScreenProps {
  api: PrivacyApi;
  providers: readonly SignInProvider[];
  store: ExportStore;
  /** API base URL without a trailing slash; the server's single-use `download_path` is appended to it. */
  baseUrl: () => string;
  /** Hands the single-use download URL to the system (the browser downloads the ZIP). */
  openUrl: (url: string) => Promise<unknown>;
  onBack: () => void;
  now?: () => number;
}

/**
 * Settings → Download your data (US-30, T-155). Request (re-auth) → the 48-hour wait the server imposes
 * (cancellable without re-auth) → check and download (re-auth: every read issues a fresh single-use link that
 * the system browser opens). The server enforces the delay, the 24-hour cooldown and the single use; the screen
 * only shows what it was told, using the times remembered from the request to show the wait.
 */
export function DataExportScreen({
  api,
  providers,
  store,
  baseUrl,
  openUrl,
  onBack,
  now = Date.now,
}: DataExportScreenProps) {
  const theme = useTheme();
  const toast = useToast();
  const queryClient = useQueryClient();
  const reauth = useReauth(api, providers);
  const [notice, setNotice] = useState<string | null>(null);
  const remembered = useQuery({ queryKey: EXPORT_QUERY_KEY, queryFn: () => store.load() });

  const remember = async (item: DataExport | RememberedExport | null) => {
    if (item) await store.save(item).catch(() => undefined);
    else await store.clear().catch(() => undefined);
    queryClient.setQueryData(EXPORT_QUERY_KEY, item ? toRemembered(item) : null);
  };

  const onError = (error: unknown) => {
    if (sessionEnded(error)) return;
    if (error instanceof PrivacyError && error.kind === 'cooldown') {
      const hours = Math.max(1, Math.ceil((error.retryAfterSeconds ?? 0) / HOUR_SECONDS));
      setNotice(t('dataExport.cooldown', { hours }));
      return;
    }
    toast.show({ message: privacyErrorMessage(error), variant: 'error' });
  };

  const request = useMutation({
    mutationFn: async () => {
      const grant = await reauth.request({
        title: t('dataExport.reauth.title'),
        body: t('dataExport.reauth.request'),
      });
      return grant ? api.requestExport(grant) : null;
    },
    onSuccess: async (item) => {
      if (!item) return;
      setNotice(null);
      await remember(item);
    },
    onError,
  });

  const cancel = useMutation({
    mutationFn: (id: string) => api.cancelExport(id),
    onSuccess: async () => {
      setNotice(t('dataExport.cancelled'));
      await remember(null);
    },
    onError: async (error) => {
      if (error instanceof PrivacyError && error.kind === 'not_found') {
        // Already being built (or gone): the next check tells which.
        setNotice(t('dataExport.cancelTooLate'));
        return;
      }
      onError(error);
    },
  });

  const download = useMutation({
    mutationFn: async (id: string) => {
      const grant = await reauth.request({
        title: t('dataExport.reauth.title'),
        body: t('dataExport.reauth.download'),
      });
      return grant ? api.getExport(id, grant) : null;
    },
    onSuccess: async (item) => {
      if (!item) return;
      if (item.status === 'ready' && item.downloadPath) {
        await remember(item);
        await openUrl(`${baseUrl()}${item.downloadPath}`);
        setNotice(t('dataExport.downloadStarted'));
        return;
      }
      const finished = FINISHED[item.status];
      if (finished) {
        await remember(null);
        setNotice(t(finished));
        return;
      }
      await remember(item);
      setNotice(t('dataExport.stillBuilding'));
    },
    onError: async (error) => {
      if (error instanceof PrivacyError && error.kind === 'not_found') {
        await remember(null);
        setNotice(t('dataExport.finished.expired'));
        return;
      }
      onError(error);
    },
  });

  const busy = request.isPending || cancel.isPending || download.isPending;
  const current = remembered.data ?? null;

  let body;
  if (remembered.isPending) {
    body = (
      <ActivityIndicator
        color={theme.color.primary.base}
        accessibilityLabel={t('dataExport.loading')}
      />
    );
  } else if (!current || FINISHED[current.status]) {
    body = (
      <>
        <Text tone="muted">{t('dataExport.timing')}</Text>
        <Button
          block
          label={t('dataExport.request')}
          loading={request.isPending}
          disabled={busy}
          onPress={() => request.mutate()}
        />
      </>
    );
  } else {
    const waiting = current.status === 'queued' && now() < current.availableAt;
    body = (
      <>
        <View
          testID="data-export-status"
          style={[
            styles.status,
            { backgroundColor: theme.color.surface.raised, borderColor: theme.hairline },
          ]}
        >
          <Text variant="label.md">
            {t('dataExport.requestedAt', { date: formatDateTime(current.requestedAt) })}
          </Text>
          <Text tone="muted">
            {waiting
              ? t('dataExport.waiting', { date: formatDateTime(current.availableAt) })
              : t('dataExport.checkReady')}
          </Text>
        </View>
        {waiting ? null : (
          <Button
            block
            label={t('dataExport.download')}
            loading={download.isPending}
            disabled={busy}
            onPress={() => download.mutate(current.id)}
          />
        )}
        {current.status === 'queued' ? (
          <Button
            block
            variant="secondary"
            label={t('dataExport.cancel')}
            loading={cancel.isPending}
            disabled={busy}
            onPress={() => cancel.mutate(current.id)}
          />
        ) : null}
      </>
    );
  }

  return (
    <Screen title={t('dataExport.title')} testID="data-export">
      <Text>{t('dataExport.intro')}</Text>
      {body}
      {notice ? (
        <Text accessibilityRole="alert" accessibilityLiveRegion="polite">
          {notice}
        </Text>
      ) : null}
      <Button block variant="ghost" label={t('common.back')} disabled={busy} onPress={onBack} />
      {reauth.sheet}
    </Screen>
  );
}

const toRemembered = ({
  id,
  status,
  requestedAt,
  availableAt,
}: DataExport | RememberedExport): RememberedExport => ({ id, status, requestedAt, availableAt });

const styles = StyleSheet.create({
  status: {
    padding: space[4],
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
    gap: space[1],
  },
});
