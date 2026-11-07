import { useAuthClient } from '../auth';
import { useRealtimeEvents } from './useRealtimeEvents';

/** Keeps the real-time stream open while signed in on the home phase (mounted by the root layout). */
export function RealtimeEvents() {
  const authClient = useAuthClient();
  useRealtimeEvents({ fetchStream: authClient.fetchStream, enabled: true });
  return null;
}
