import { useQuery } from '@tanstack/react-query';

import { apiBaseUrl } from '../auth/config';
import { fetchRemoteConfig, type RemoteConfig } from './remoteConfig';

/** Matches the API's `Cache-Control: max-age` for `/v1/config` (config/client.yaml `cache_max_age_secs`). */
const STALE_MS = 5 * 60_000;

/**
 * The current remote config, or undefined while loading or offline. Without it the app shows every entry the
 * device supports (kill switches are display-level; the server rejects killed ids on its own).
 */
export function useRemoteConfig(): RemoteConfig | undefined {
  const { data } = useQuery({
    queryKey: ['remote-config'],
    queryFn: () => fetchRemoteConfig(apiBaseUrl()),
    staleTime: STALE_MS,
  });
  return data;
}
