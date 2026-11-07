import { AuthError } from '../../features/auth/authClient';
import { isServerBusy } from '../serverBusy';

it('recognises 503 SERVER_BUSY on any API error and nothing else', () => {
  expect(isServerBusy(new AuthError('unavailable', 503, 'SERVER_BUSY'))).toBe(true);
  expect(isServerBusy({ kind: 'busy', status: 503 })).toBe(true);
  expect(isServerBusy(new AuthError('unavailable', 503, 'TEMPORARILY_UNAVAILABLE'))).toBe(false);
  expect(isServerBusy(new AuthError('unavailable'))).toBe(false);
  expect(isServerBusy({ status: 429, code: 'SERVER_BUSY' })).toBe(false);
  expect(isServerBusy(null)).toBe(false);
  expect(isServerBusy('SERVER_BUSY')).toBe(false);
});
