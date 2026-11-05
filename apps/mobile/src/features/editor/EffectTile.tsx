import { Image, Pressable, StyleSheet, View } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import { Text, useTheme } from '../../ui';
import type { PickerTile, UnavailableReason } from './pickerTabs';
import type { ThumbSize } from './tileThumbnails';

const REASON_KEYS: Record<UnavailableReason, MessageKey> = {
  needsUpdate: 'editor.tile.reason.needsUpdate',
  osTooOld: 'editor.tile.reason.osTooOld',
  notSupported: 'editor.tile.reason.notSupported',
};

/** Short visible reason a tile cannot be used (also read out by screen readers). */
export const unavailableReason = (reason: UnavailableReason) => t(REASON_KEYS[reason]);

/** Accessibility label of a tile: the style name, plus its state when it is not plainly available. */
export function tileLabel(tile: PickerTile): string {
  const name = t(tile.entry.name as MessageKey);
  switch (tile.state.kind) {
    case 'unavailable':
      return t('editor.tile.unavailable.a11y', {
        name,
        reason: unavailableReason(tile.state.reason),
      });
    case 'locked':
      return t('editor.tile.locked.a11y', { name, unlock: tile.state.unlock });
    case 'available':
      return tile.isNew ? t('editor.tile.new.a11y', { name }) : name;
  }
}

export interface EffectTileProps {
  tile: PickerTile;
  selected: boolean;
  /** Baked preview (PNG data URI); a placeholder shows until it is ready. */
  thumbnail: string | undefined;
  size: ThumbSize;
  onPress: () => void;
}

/**
 * Picker tile (design-system "Effect tile"): a baked preview of the style on the current photo, its name and
 * its state: selected, new, locked (ad or pro unlock) or unavailable on this card or device with the reason.
 * Locked and unavailable tiles stay pressable so the screen can explain why they cannot be used.
 */
export function EffectTile({ tile, selected, thumbnail, size, onPress }: EffectTileProps) {
  const theme = useTheme();
  const { state } = tile;
  const usable = state.kind === 'available';
  return (
    <Pressable
      testID={`tile-${tile.entry.id}`}
      accessibilityRole="button"
      accessibilityLabel={tileLabel(tile)}
      accessibilityHint={t(tile.entry.description as MessageKey)}
      accessibilityState={{ selected, disabled: !usable }}
      onPress={onPress}
      style={[
        styles.tile,
        {
          minWidth: theme.touchMin,
          borderColor: selected ? theme.color.primary.base : theme.hairline,
          borderWidth: selected ? 2 : StyleSheet.hairlineWidth,
          backgroundColor: theme.color.surface.raised,
        },
      ]}
    >
      <View style={[size, styles.thumb, { backgroundColor: theme.color.bg.canvas }]}>
        {thumbnail ? (
          <Image
            testID={`tile-thumb-${tile.entry.id}`}
            source={{ uri: thumbnail }}
            style={[size, !usable && styles.dimmed]}
            accessibilityIgnoresInvertColors
          />
        ) : null}
      </View>
      <Text variant="label.sm" style={styles.name}>
        {t(tile.entry.name as MessageKey)}
      </Text>
      {state.kind === 'locked' ? (
        <Text variant="label.sm" tone="primary">
          {t('editor.tile.locked', { unlock: state.unlock })}
        </Text>
      ) : null}
      {state.kind === 'unavailable' ? (
        <Text variant="label.sm" tone="muted" style={[styles.name, { maxWidth: size.width * 1.6 }]}>
          {unavailableReason(state.reason)}
        </Text>
      ) : null}
      {usable && tile.isNew ? (
        <Text variant="label.sm" tone="primary">
          {t('editor.tile.new')}
        </Text>
      ) : null}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  tile: {
    alignItems: 'center',
    gap: space[1],
    padding: space[2],
    borderRadius: radius.md,
  },
  thumb: { borderRadius: radius.sm, overflow: 'hidden' },
  dimmed: { opacity: 0.4 },
  name: { textAlign: 'center' },
});
