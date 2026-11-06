import { useMutation } from '@tanstack/react-query';
import { useState } from 'react';

import { t, type MessageKey } from '../../i18n';
import { Button, Sheet, Text, useToast } from '../../ui';
import { AuthError } from '../auth';
import { newIdempotencyKey } from '../friends/idempotencyKey';
import {
  REPORT_REASONS,
  ReportsError,
  type ReportReason,
  type ReportsApi,
  type ReportTarget,
} from './reportsApi';

const REASON_LABELS: Record<ReportReason, MessageKey> = {
  sexual: 'report.reason.sexual',
  minor_safety: 'report.reason.minorSafety',
  harassment: 'report.reason.harassment',
  violence: 'report.reason.violence',
  hate: 'report.reason.hate',
  spam: 'report.reason.spam',
  other: 'report.reason.other',
};

const TITLES: Record<ReportTarget['kind'], MessageKey> = {
  card: 'report.title.card',
  board: 'report.title.board',
  public_link: 'report.title.link',
  user: 'report.title.user',
};

export interface ReportSheetProps {
  api: ReportsApi;
  /** The thing to report; null keeps the sheet closed. */
  target: ReportTarget | null;
  onClose: () => void;
}

/**
 * Report sheet (US-26, AC-26.1): the caller's "Report" button is the first tap, a reason is the second and sends it,
 * so reporting takes two taps. The server checks that the target is visible to the reporter (else 404), deduplicates
 * per target and limits reports per day; the sheet only says what happened. The reported person is never told.
 */
export function ReportSheet({ api, target, onClose }: ReportSheetProps) {
  const toast = useToast();
  // One key per opened sheet: a retry after a lost response is the same report.
  const [key, setKey] = useState(newIdempotencyKey);
  const [openedFor, setOpenedFor] = useState(target);
  if (target !== openedFor) {
    setOpenedFor(target);
    setKey(newIdempotencyKey());
  }

  const send = useMutation({
    mutationFn: ({ to, reason }: { to: ReportTarget; reason: ReportReason }) =>
      api.report(to, reason, key),
    onSuccess: () => {
      toast.show({ message: t('report.sent'), variant: 'success' });
      onClose();
    },
    onError: (error) => {
      if (error instanceof AuthError && error.kind !== 'unavailable') return; // signed out
      const message =
        error instanceof ReportsError && error.kind === 'not_found'
          ? t('report.error.gone')
          : (error instanceof ReportsError && error.kind === 'rate_limited') ||
              (error instanceof AuthError && error.status === 429)
            ? t('report.error.rateLimited')
            : t('report.error.generic');
      toast.show({ message, variant: 'error' });
      if (error instanceof ReportsError && error.kind === 'not_found') onClose();
    },
  });

  return (
    <Sheet
      visible={target !== null}
      onClose={() => {
        if (!send.isPending) onClose();
      }}
      title={target ? t(TITLES[target.kind]) : ''}
      testID="report-sheet"
    >
      <Text tone="muted">{t('report.intro')}</Text>
      {REPORT_REASONS.map((reason) => (
        <Button
          key={reason}
          block
          variant="secondary"
          label={t(REASON_LABELS[reason])}
          loading={send.isPending && send.variables?.reason === reason}
          disabled={send.isPending && send.variables?.reason !== reason}
          onPress={() => {
            if (target) send.mutate({ to: target, reason });
          }}
        />
      ))}
    </Sheet>
  );
}
