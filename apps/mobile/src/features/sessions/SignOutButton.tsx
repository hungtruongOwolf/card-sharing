import { useMutation } from '@tanstack/react-query';

import { t } from '../../i18n';
import { Button, useToast, type ButtonVariant } from '../../ui';

export interface SignOutButtonProps {
  /**
   * The auth client's `signOut`: asks the server to revoke this device's session (`DELETE /v1/sessions/current`,
   * best effort within a few seconds) and always clears the tokens and device data here.
   */
  signOut: () => Promise<void>;
  variant?: ButtonVariant;
  block?: boolean;
}

/**
 * "Sign out of this device" (sprint 10 B8). Only this device: other devices stay signed in (the sessions screen has
 * "Log out everywhere" for that). Once signed out, the onboarding guard moves the app to sign-in by itself.
 */
export function SignOutButton({ signOut, variant = 'secondary', block }: SignOutButtonProps) {
  const toast = useToast();
  const leave = useMutation({
    mutationFn: signOut,
    onError: () => toast.show({ message: t('sessions.signOutHere.failed'), variant: 'error' }),
  });
  return (
    <Button
      block={block}
      variant={variant}
      label={t('sessions.signOutHere')}
      accessibilityHint={t('sessions.signOutHere.hint')}
      loading={leave.isPending}
      onPress={() => leave.mutate()}
    />
  );
}
