import { useMemo, useState, type ReactElement } from 'react';

import { useAuthClient } from '../auth';
import { createReportsApi, type ReportsApi, type ReportTarget } from './reportsApi';
import { ReportSheet } from './ReportSheet';

export interface ReportSheetHandle {
  /** Opens the reason sheet for `target` (the first of the two taps). */
  open(target: ReportTarget): void;
  /** True while the sheet is up (a story behind it pauses). */
  isOpen: boolean;
  /** Render once in the screen. */
  sheet: ReactElement;
}

/** A report sheet over the signed-in auth client (or a given API in tests). */
export function useReportSheet(api?: ReportsApi): ReportSheetHandle {
  const client = useAuthClient();
  const reports = useMemo(() => api ?? createReportsApi(client.fetch), [api, client]);
  const [target, setTarget] = useState<ReportTarget | null>(null);
  return {
    open: setTarget,
    isOpen: target !== null,
    sheet: <ReportSheet api={reports} target={target} onClose={() => setTarget(null)} />,
  };
}
