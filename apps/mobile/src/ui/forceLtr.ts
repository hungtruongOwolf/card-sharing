import { Platform, type PlatformOSType, type ViewProps } from 'react-native';

/**
 * View props that lay a subtree out left to right whatever the app language (for overlays whose
 * coordinates are physical, such as a canvas). Native reads the `direction` style; react-native-web
 * rejects that style ("did you mean writingDirection?") and reads the `dir` prop instead, which also
 * makes the children resolve `start` as left.
 */
export function forceLtrProps(os: PlatformOSType = Platform.OS): Pick<ViewProps, 'style'> {
  return os === 'web'
    ? ({ dir: 'ltr' } as Pick<ViewProps, 'style'>)
    : { style: { direction: 'ltr' } };
}
