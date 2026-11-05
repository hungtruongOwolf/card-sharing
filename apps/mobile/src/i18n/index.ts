import { I18nManager } from 'react-native';

import { formatMessage, MessageFormatError, type MessageValues } from './format';
import { en, type Catalog, type MessageKey } from './messages/en';

export type { MessageKey } from './messages/en';
export type { MessageValues } from './format';

/** Registered locales. Add a catalog here (typed against the English keys) to ship a translation. */
const catalogs: Record<string, Catalog> = { en };

export const DEFAULT_LOCALE = 'en';
let activeLocale = DEFAULT_LOCALE;

export function setLocale(locale: string): void {
  activeLocale = locale in catalogs ? locale : DEFAULT_LOCALE;
}

export function getLocale(): string {
  return activeLocale;
}

/** True when the OS lays the app out right-to-left; use it only for direction-dependent artwork. */
export function isRTL(): boolean {
  return I18nManager.isRTL;
}

/**
 * Translates `key` with ICU `values`. In development a broken message throws so it is fixed;
 * in production the key is shown instead of crashing the screen.
 */
export function t(key: MessageKey, values?: MessageValues): string {
  const message = catalogs[activeLocale]?.[key] ?? en[key];
  try {
    return formatMessage(message, values, activeLocale);
  } catch (error) {
    if (__DEV__ || !(error instanceof MessageFormatError)) throw error;
    return key;
  }
}
