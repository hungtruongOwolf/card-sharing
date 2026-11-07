import { addEventListener, type NetInfoState } from '@react-native-community/netinfo';

/** Calls `onOnline` each time the device comes back online; returns the unsubscribe. */
export type OnlineSource = (onOnline: () => void) => () => void;

/** Online = connected, and not known to be without internet (`isInternetReachable` is null until probed). */
export const isOnline = (state: Pick<NetInfoState, 'isConnected' | 'isInternetReachable'>) =>
  state.isConnected === true && state.isInternetReachable !== false;

/**
 * Back-online signal from `@react-native-community/netinfo` (#265; on the web it follows the browser's online
 * events). Only an offline → online transition fires, so repeated updates while online (a Wi-Fi strength change,
 * the reachability probe) never trigger a burst of retries. The first state only sets the baseline.
 */
export const netInfoOnlineSource: OnlineSource = (onOnline) => {
  let online: boolean | null = null;
  return addEventListener((state) => {
    const now = isOnline(state);
    if (online === false && now) onOnline();
    online = now;
  });
};
