import { Text } from '../../../ui';

/** Form-level error, announced by screen readers when it appears. */
export function FormMessage({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <Text
      variant="body.md"
      tone="danger"
      accessibilityRole="alert"
      accessibilityLiveRegion="polite"
    >
      {message}
    </Text>
  );
}
