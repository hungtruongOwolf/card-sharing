import { Platform } from 'react-native';

import {
  visibleEntries,
  type Capability,
  type DeviceContext,
  type EffectEntry,
  type Registry,
  type RemoteRegistryConfig,
} from '@card-sharing/effects-registry';

import type { MessageKey } from '../../i18n';
import { hasCutRenderer } from '../cut-styles';
import { hasFinishRenderer } from './drawFinish';
import type { StyleSlot } from './editorState';

/**
 * Picker tabs of the editor, built from registry data only (AC-10.2): a new entry shows up without screen
 * changes. Entries are routed by `resolveAvailability` (kill switch, feature flag, app/OS version, device
 * capabilities), so a whole-photo card lists only the styles that work without a subject mask.
 */
export interface PickerTab {
  slot: StyleSlot;
  labelKey: MessageKey;
  /** Finishes are optional; cut styles always have a selection. */
  optional: boolean;
  entries: EffectEntry[];
}

/** Built-in entries (tag `builtIn`, e.g. paper lift) apply to every card and are never picker tiles. */
const isPickable = (entry: EffectEntry) => !entry.tags.includes('builtIn');

export function pickerTabs(
  registry: Registry,
  device: DeviceContext,
  config: RemoteRegistryConfig | undefined,
): PickerTab[] {
  const visible = (entries: EffectEntry[]) =>
    visibleEntries(entries.filter(isPickable), device, config).map(({ entry }) => entry);
  return [
    {
      slot: 'cut',
      labelKey: 'editor.tab.cut',
      optional: false,
      entries: visible(registry.byKind('cutStyle').filter(hasCutRenderer)),
    },
    {
      slot: 'finish',
      labelKey: 'editor.tab.finish',
      optional: true,
      entries: visible(registry.byTag('finish').filter(hasFinishRenderer)),
    },
  ];
}

/**
 * The default cut style: with a subject, the first sticker-style cut (one that traces the subject contour,
 * "anything becomes a sticker"); otherwise the first listed style.
 */
export function defaultCut(
  entries: readonly EffectEntry[],
  hasSubject: boolean,
): EffectEntry | undefined {
  const sticker = hasSubject
    ? entries.find((entry) => entry.inputs.includes('contour'))
    : undefined;
  return sticker ?? entries[0];
}

/**
 * This device as the registry sees it. `subjectMask` is present only when the current card has a subject
 * (a mask exists and the user did not choose "Use whole photo").
 */
export function deviceContext(appVersion: string, hasSubject: boolean): DeviceContext {
  // Other capabilities (pose, landmarks, gyro) are probed when the first entry needing them ships.
  const capabilities = new Set<Capability>(hasSubject ? ['subjectMask'] : []);
  return {
    platform: Platform.OS === 'android' ? 'android' : 'ios',
    osVersion: Platform.Version,
    appVersion,
    capabilities,
  };
}
