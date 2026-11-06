import type { QueryClient } from '@tanstack/react-query';

import { isTimeZone, type NotificationPrefs, type NotificationsApi } from './notificationsApi';

/** Query key of my notification preferences (shared by the settings screen and the time-zone sync). */
export const NOTIFICATION_PREFS_QUERY_KEY = ['notification-preferences'] as const;

/**
 * The device's IANA time zone (`Europe/Paris`), or null where the runtime reports none or something that is not
 * an IANA name (old Hermes builds without full `Intl`).
 */
export function deviceTimeZone(): string | null {
  try {
    const zone: unknown = Intl.DateTimeFormat().resolvedOptions().timeZone;
    return isTimeZone(zone) ? zone : null;
  } catch {
    return null;
  }
}

/**
 * Makes the server apply quiet hours in `zone` (T-87, AC-31.1: "22:00–08:00 local"): reads my preferences and,
 * only when the stored zone differs, writes them back unchanged except for the zone. Returns what the server
 * holds afterwards and refreshes the settings screen's copy.
 */
export async function syncTimeZone(
  api: NotificationsApi,
  zone: string,
  queryClient: QueryClient,
): Promise<NotificationPrefs> {
  const current = await api.get();
  const stored =
    current.timeZone === zone ? current : await api.put({ ...current, timeZone: zone });
  queryClient.setQueryData(NOTIFICATION_PREFS_QUERY_KEY, stored);
  return stored;
}
