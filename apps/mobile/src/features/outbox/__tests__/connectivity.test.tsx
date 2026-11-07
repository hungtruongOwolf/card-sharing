import { addEventListener, type NetInfoState } from '@react-native-community/netinfo';
import { act, render } from '@testing-library/react-native';
import { Text } from 'react-native';

import { isOnline, netInfoOnlineSource, type OnlineSource } from '../connectivity';
import type { Outbox } from '../outbox';
import { OutboxProvider } from '../OutboxProvider';

const state = (isConnected: boolean | null, isInternetReachable: boolean | null) =>
  ({ isConnected, isInternetReachable }) as NetInfoState;

describe('netInfoOnlineSource', () => {
  it('fires only on an offline to online transition', () => {
    let emit: (next: NetInfoState) => void = () => undefined;
    const unsubscribe = jest.fn();
    jest.mocked(addEventListener).mockImplementation((listener) => {
      emit = listener;
      return unsubscribe;
    });
    const onOnline = jest.fn();
    const stop = netInfoOnlineSource(onOnline);

    emit(state(true, true)); // baseline: already online
    emit(state(true, null));
    expect(onOnline).not.toHaveBeenCalled();
    emit(state(false, false));
    emit(state(true, null)); // reachability not probed yet counts as online
    expect(onOnline).toHaveBeenCalledTimes(1);
    emit(state(true, true));
    expect(onOnline).toHaveBeenCalledTimes(1);
    emit(state(true, false)); // connected to a network without internet
    emit(state(true, true));
    expect(onOnline).toHaveBeenCalledTimes(2);

    stop();
    expect(unsubscribe).toHaveBeenCalled();
  });

  it('reads online as connected and not known to be unreachable', () => {
    expect(isOnline(state(true, true))).toBe(true);
    expect(isOnline(state(true, null))).toBe(true);
    expect(isOnline(state(true, false))).toBe(false);
    expect(isOnline(state(null, null))).toBe(false);
  });
});

describe('OutboxProvider', () => {
  it('runs every pending entry when the device comes back online, and stops listening on unmount', async () => {
    const outbox = {
      list: jest.fn().mockResolvedValue([]),
      nextDueAt: jest.fn().mockResolvedValue(null),
      process: jest.fn().mockResolvedValue(undefined),
      processNow: jest.fn().mockResolvedValue(undefined),
      subscribe: jest.fn(() => () => undefined),
    } as unknown as jest.Mocked<Outbox>;
    let online: () => void = () => undefined;
    const stop = jest.fn();
    const onlineSource: OnlineSource = (listener) => {
      online = listener;
      return stop;
    };
    const view = await render(
      <OutboxProvider outbox={outbox} onlineSource={onlineSource}>
        <Text>child</Text>
      </OutboxProvider>,
    );
    expect(outbox.processNow).not.toHaveBeenCalled();
    await act(async () => online());
    expect(outbox.processNow).toHaveBeenCalledTimes(1);
    await view.unmount();
    expect(stop).toHaveBeenCalled();
  });
});
