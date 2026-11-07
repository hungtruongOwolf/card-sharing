import { Platform, type PlatformOSType } from 'react-native';

import {
  EFFECT_SLOT_KINDS,
  killedIds,
  missingCapabilities,
  resolveAvailability,
  type Capability,
  type DeviceContext,
  type EffectEntry,
  type Registry,
  type RemoteRegistryConfig,
} from '@card-sharing/effects-registry';

import type { MessageKey } from '../../i18n';
import { hasCutRenderer } from '../cut-styles';
import { hasEffectRenderer } from '../effects';
import { hasFinishRenderer } from './drawFinish';
import type { StyleSlot } from './editorState';

/** A picker tab: one cut style, one optional finish, or a stack of effects. */
export type PickerSlot = StyleSlot | 'effects';

/** Why a tile is shown but cannot be used on this card or device (design-system "Effect tile"). */
export type UnavailableReason =
  /** The entry needs a newer app build (`flags.minAppVersion`). */
  | 'needsUpdate'
  /** The OS is older than the entry's minimum (native only: the web target has no OS gate). */
  | 'osTooOld'
  /**
   * A device capability other than the subject mask is missing (pose, landmarks, gyro), including a native-only
   * feature on the web target.
   */
  | 'notSupported';

export type TileState =
  | { kind: 'available' }
  /** Not free (`unlock` ad or pro). Not selectable until the unlock flow ships (monetization). */
  | { kind: 'locked'; unlock: 'ad' | 'pro' }
  | { kind: 'unavailable'; reason: UnavailableReason };

export interface PickerTile {
  entry: EffectEntry;
  state: TileState;
  /** Registry tag `new`: shows a "New" badge. */
  isNew: boolean;
}

/**
 * Picker tabs of the editor, built from registry data only (AC-10.2): a new entry shows up without screen
 * changes. Entries are routed by `resolveAvailability`: kill-switched and dark-launched entries are never shown,
 * and neither are subject styles on a whole-photo card (EM decision 2026-11-05, effects-catalog); entries the
 * device cannot use for another reason are shown as unavailable with the reason; non-free entries are locked.
 */
export interface PickerTab {
  slot: PickerSlot;
  labelKey: MessageKey;
  /** Finishes and effects are optional; cut styles always have a selection. */
  optional: boolean;
  /** Every tile the picker shows, in registry order. */
  tiles: PickerTile[];
  /** The selectable entries (available tiles only), in registry order. */
  entries: EffectEntry[];
}

/** Built-in entries (tag `builtIn`, e.g. paper lift) apply to every card and are never picker tiles. */
const isPickable = (entry: EffectEntry) => !entry.tags.includes('builtIn');

/** Tile state of one entry, or null when the picker must not show it at all. */
export function tileState(
  entry: EffectEntry,
  device: DeviceContext,
  config: RemoteRegistryConfig | undefined,
  killed: ReadonlySet<string> = killedIds(config),
): TileState | null {
  const availability = resolveAvailability(entry, device, config, killed);
  if (availability.status === 'hidden') {
    switch (availability.reason) {
      // Switched off or not launched yet: invisible, never teased.
      case 'killSwitch':
      case 'featureFlag':
        return null;
      case 'minAppVersion':
        return { kind: 'unavailable', reason: 'needsUpdate' };
      case 'os':
        return { kind: 'unavailable', reason: 'osTooOld' };
      case 'requires': {
        const missing = missingCapabilities(entry, device);
        // No subject mask = a whole-photo card, which offers only mask-free styles (EM decision 2026-11-05).
        if (missing.includes('subjectMask')) return null;
        return { kind: 'unavailable', reason: 'notSupported' };
      }
    }
  }
  if (entry.unlock !== 'free') return { kind: 'locked', unlock: entry.unlock };
  return { kind: 'available' };
}

export function pickerTabs(
  registry: Registry,
  device: DeviceContext,
  config: RemoteRegistryConfig | undefined,
): PickerTab[] {
  const killed = killedIds(config);
  const tab = (
    slot: PickerSlot,
    labelKey: MessageKey,
    optional: boolean,
    candidates: EffectEntry[],
  ): PickerTab => {
    const tiles = candidates.filter(isPickable).flatMap((entry) => {
      const state = tileState(entry, device, config, killed);
      return state ? [{ entry, state, isNew: entry.tags.includes('new') }] : [];
    });
    return {
      slot,
      labelKey,
      optional,
      tiles,
      entries: tiles.filter((t) => t.state.kind === 'available').map((t) => t.entry),
    };
  };
  const effects = tab('effects', 'editor.tab.effects', true, registry.entries.filter(isEffect));
  return [
    tab('cut', 'editor.tab.cut', false, registry.byKind('cutStyle').filter(hasCutRenderer)),
    tab('finish', 'editor.tab.finish', true, registry.byTag('finish').filter(hasFinishRenderer)),
    // Shown once at least one effect can be listed (T-114).
    ...(effects.tiles.length > 0 ? [effects] : []),
  ];
}

/**
 * Entries the Effects tab lists: stackable kinds (`EFFECT_SLOT_KINDS`) other than the finishes, which have their
 * own tab, with a renderer this build has (filters reuse the finish renderers).
 */
function isEffect(entry: EffectEntry): boolean {
  if (!EFFECT_SLOT_KINDS.includes(entry.kind) || entry.tags.includes('finish')) return false;
  return entry.kind === 'filter' ? hasFinishRenderer(entry) : hasEffectRenderer(entry);
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
 * (a mask exists and the user did not choose "Use whole photo"). The web target (ADR-0009) is its own platform:
 * react-native-web has no `Platform.Version`, and a browser must not be judged against iOS or Android minimums.
 */
export function deviceContext(
  appVersion: string,
  hasSubject: boolean,
  os: PlatformOSType = Platform.OS,
  osVersion: string | number | undefined = Platform.Version,
): DeviceContext {
  // Other capabilities (pose, landmarks, gyro) are probed when the first entry needing them ships.
  const capabilities = new Set<Capability>(hasSubject ? ['subjectMask'] : []);
  if (os === 'web') return { platform: 'web', appVersion, capabilities };
  return {
    platform: os === 'android' ? 'android' : 'ios',
    // An unknown version compares as 0, which hides OS-gated entries rather than guessing they run.
    osVersion: osVersion ?? 0,
    appVersion,
    capabilities,
  };
}
