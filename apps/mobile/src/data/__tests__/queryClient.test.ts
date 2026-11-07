import { NotificationsError } from '../../features/notifications/notificationsApi';
import { isRateLimited, retryQuery } from '../queryClient';

describe('query retries (B7)', () => {
  it('retries a failed query once', () => {
    const error = new NotificationsError('unavailable', 503);
    expect(retryQuery(0, error)).toBe(true);
    expect(retryQuery(1, error)).toBe(false);
  });

  it('never retries a rate-limited query', () => {
    expect(retryQuery(0, new NotificationsError('rate_limited', 429))).toBe(false);
    expect(retryQuery(0, { status: 429 })).toBe(false);
    expect(retryQuery(0, { kind: 'rate_limited' })).toBe(false);
  });

  it('recognises a 429 on any client error shape only', () => {
    expect(isRateLimited(new NotificationsError('rate_limited', 429, 5000))).toBe(true);
    expect(isRateLimited(new Error('429'))).toBe(false);
    expect(isRateLimited(null)).toBe(false);
  });
});
