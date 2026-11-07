import { datePartOrder, EMPTY_BIRTH_DATE, isBirthDateComplete, toIsoBirthDate } from '../birthDate';
import { toCameraPermissionStatus } from '../cameraPermissions';
import { resolvePhase, type PhaseInput } from '../phase';
import { displayNameIssue, graphemeCount, isValidHandle, normalizeHandle } from '../validation';

const TODAY = new Date(2026, 10, 5);

describe('toIsoBirthDate', () => {
  it('accepts any real date from 1900 to today, whatever the age (age-blind, AC-2.4)', () => {
    expect(toIsoBirthDate({ day: '5', month: '11', year: '2026' }, TODAY)).toBe('2026-11-05');
    expect(toIsoBirthDate({ day: '1', month: '1', year: '2020' }, TODAY)).toBe('2020-01-01');
    expect(toIsoBirthDate({ day: '29', month: '02', year: '2008' }, TODAY)).toBe('2008-02-29');
    expect(toIsoBirthDate({ day: '01', month: '01', year: '1900' }, TODAY)).toBe('1900-01-01');
  });

  it('rejects impossible, future and out-of-range dates', () => {
    expect(toIsoBirthDate({ day: '6', month: '11', year: '2026' }, TODAY)).toBeNull();
    expect(toIsoBirthDate({ day: '29', month: '2', year: '2007' }, TODAY)).toBeNull();
    expect(toIsoBirthDate({ day: '31', month: '4', year: '2000' }, TODAY)).toBeNull();
    expect(toIsoBirthDate({ day: '1', month: '13', year: '2000' }, TODAY)).toBeNull();
    expect(toIsoBirthDate({ day: '0', month: '1', year: '2000' }, TODAY)).toBeNull();
    expect(toIsoBirthDate({ day: '1', month: '1', year: '1899' }, TODAY)).toBeNull();
    expect(toIsoBirthDate({ day: '1', month: '1', year: '99' }, TODAY)).toBeNull();
    expect(toIsoBirthDate(EMPTY_BIRTH_DATE, TODAY)).toBeNull();
  });

  it('is complete only with a four-digit year', () => {
    expect(isBirthDateComplete({ day: '1', month: '1', year: '200' })).toBe(false);
    expect(isBirthDateComplete({ day: '1', month: '1', year: '2000' })).toBe(true);
  });
});

describe('datePartOrder', () => {
  it('follows the locale and falls back to month/day/year', () => {
    expect(datePartOrder('en-US')).toEqual(['month', 'day', 'year']);
    expect(datePartOrder('en-GB')).toEqual(['day', 'month', 'year']);
    expect(datePartOrder('not a locale!')).toEqual(['month', 'day', 'year']);
  });
});

describe('profile validation', () => {
  it('counts graphemes for display names (2-30)', () => {
    expect(graphemeCount('👩‍👩‍👧')).toBe(1);
    expect(displayNameIssue(' a ')).toBe('tooShort');
    expect(displayNameIssue('Mai')).toBeNull();
    expect(displayNameIssue('x'.repeat(31))).toBe('tooLong');
  });

  it('accepts only lowercase handles of 3-20 [a-z0-9._]', () => {
    expect(normalizeHandle('  Mai.Pet ')).toBe('mai.pet');
    expect(isValidHandle('mai_pet.2')).toBe(true);
    expect(isValidHandle('ma')).toBe(false);
    expect(isValidHandle('Mai')).toBe(false);
    expect(isValidHandle('mai-pet')).toBe(false);
    expect(isValidHandle('a'.repeat(21))).toBe(false);
  });
});

describe('toCameraPermissionStatus', () => {
  it('maps Expo permission responses', () => {
    expect(toCameraPermissionStatus({ granted: true, status: 'granted', canAskAgain: true })).toBe(
      'granted',
    );
    expect(
      toCameraPermissionStatus({ granted: false, status: 'undetermined', canAskAgain: true }),
    ).toBe('undetermined');
    expect(toCameraPermissionStatus({ granted: false, status: 'denied', canAskAgain: true })).toBe(
      'deniedCanAsk',
    );
    expect(toCameraPermissionStatus({ granted: false, status: 'denied', canAskAgain: false })).toBe(
      'blocked',
    );
  });
});

describe('resolvePhase', () => {
  const base: PhaseInput = {
    auth: 'signedIn',
    step: undefined,
    meFailed: false,
    refused: false,
    displayNameSaved: false,
    cameraHandled: false,
    cameraGranted: false,
  };

  it('follows auth and the server step', () => {
    expect(resolvePhase({ ...base, auth: 'loading' })).toBe('loading');
    expect(resolvePhase({ ...base, auth: 'signedOut' })).toBe('signIn');
    expect(resolvePhase(base)).toBe('loading');
    expect(resolvePhase({ ...base, meFailed: true })).toBe('error');
    expect(resolvePhase({ ...base, step: 'age' })).toBe('birthDate');
    expect(resolvePhase({ ...base, step: 'profile' })).toBe('displayName');
    expect(resolvePhase({ ...base, step: 'profile', displayNameSaved: true })).toBe('handle');
    expect(resolvePhase({ ...base, step: 'done' })).toBe('camera');
    expect(resolvePhase({ ...base, step: 'done', cameraHandled: true })).toBe('home');
  });

  it('reads the camera permission before the explainer and skips it when granted (R1)', () => {
    expect(resolvePhase({ ...base, step: 'done', cameraGranted: undefined })).toBe('loading');
    expect(resolvePhase({ ...base, step: 'done', cameraGranted: true })).toBe('home');
    expect(resolvePhase({ ...base, step: 'done', cameraGranted: false })).toBe('camera');
    expect(
      resolvePhase({ ...base, step: 'done', cameraGranted: undefined, cameraHandled: true }),
    ).toBe('home');
  });

  it('never offers the DOB screen once the server moved past the age step (AC-2.3)', () => {
    for (const step of ['profile', 'done'] as const) {
      for (const displayNameSaved of [false, true]) {
        for (const cameraHandled of [false, true]) {
          expect(resolvePhase({ ...base, step, displayNameSaved, cameraHandled })).not.toBe(
            'birthDate',
          );
        }
      }
    }
  });

  it('shows only the deletion-pending screen while the account waits out its grace period', () => {
    for (const cameraHandled of [false, true]) {
      expect(resolvePhase({ ...base, step: 'done', cameraHandled, deletionPending: true })).toBe(
        'deletionPending',
      );
    }
    expect(resolvePhase({ ...base, auth: 'signedOut', deletionPending: true })).toBe('signIn');
  });

  it('keeps the refusal screen up while and after the session ends', () => {
    expect(resolvePhase({ ...base, refused: true, step: 'age' })).toBe('refused');
    expect(resolvePhase({ ...base, refused: true, auth: 'signedOut' })).toBe('refused');
  });
});
