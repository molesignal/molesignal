import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as meApi from '@/api/me';
import { toApiError } from '@/lib/http';
import { ProductState, productStateFor } from '@/product/states';
import { AccountSection } from '@/routes/account/AccountSection';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormInput, FormTextarea } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';

const MAX_AVATAR_BYTES = 2 * 1024 * 1024;

export function AccountProfile() {
  const { t } = useTranslation(['account', 'common', 'settings-admin']);
  const queryClient = useQueryClient();
  const token = useAuthStore((state) => state.token);
  const context = useAuthStore((state) => state.ctx);
  const setSession = useAuthStore((state) => state.setSession);
  const [displayName, setDisplayName] = React.useState('');
  const [bio, setBio] = React.useState('');
  const fileInputRef = React.useRef<HTMLInputElement>(null);
  const profileQuery = useQuery({
    queryKey: ['me', 'profile'],
    queryFn: () => meApi.profile(),
  });
  const profile = profileQuery.data;

  React.useEffect(() => {
    if (!profile) return;
    setDisplayName(profile.display_name);
    setBio(profile.bio ?? '');
  }, [profile]);

  const applyProfile = React.useCallback(
    (next: meApi.MeProfile) => {
      queryClient.setQueryData(['me', 'profile'], next);
      if (token && context) {
        setSession(token, {
          ...context,
          email: next.email,
          display_name: next.display_name,
        });
      }
    },
    [context, queryClient, setSession, token],
  );

  const save = useMutation({
    mutationFn: () =>
      meApi.updateProfile({
        display_name: displayName.trim(),
        bio: bio.trim(),
      }),
    onSuccess: (next) => {
      applyProfile(next);
      toast.success(t('common:status.saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const uploadAvatar = useMutation({
    mutationFn: (file: File) => meApi.uploadAvatar(file),
    onSuccess: (next) => {
      applyProfile(next);
      toast.success(t('common:status.saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const removeAvatar = useMutation({
    mutationFn: () => meApi.updateProfile({ avatar_url: '' }),
    onSuccess: (next) => {
      applyProfile(next);
      toast.success(t('common:status.saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const pageState = productStateFor(
    profileQuery.isLoading ? 'loading' : profileQuery.isError ? 'error' : null,
    { error: profileQuery.error },
  );
  const dirty = Boolean(
    profile &&
      (displayName.trim() !== profile.display_name ||
        bio.trim() !== (profile.bio ?? '')),
  );
  const invalid = displayName.trim().length === 0;
  const avatarPending = uploadAvatar.isPending || removeAvatar.isPending;
  const initial = (displayName || profile?.display_name || 'M')[0]?.toUpperCase() ?? 'M';
  const reset = () => {
    if (!profile) return;
    setDisplayName(profile.display_name);
    setBio(profile.bio ?? '');
  };

  const pickAvatar = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = '';
    if (!file) return;
    if (file.size > MAX_AVATAR_BYTES) {
      toast.error(t('account:profile.avatar_too_large'));
      return;
    }
    uploadAvatar.mutate(file);
  };

  return (
    <AccountSection
      title={t('account:profile.title')}
      subtitle={t('account:profile.subtitle')}
    >
      {pageState ? (
        <ProductState {...pageState} compact />
      ) : (
        <form
          className="space-y-6"
          onSubmit={(event) => {
            event.preventDefault();
            if (dirty && !invalid && !save.isPending) save.mutate();
          }}
        >
          <div className="flex flex-col gap-4 pb-2 sm:flex-row sm:items-center">
            <div className="flex h-24 w-24 shrink-0 items-center justify-center overflow-hidden rounded-full border border-bd-1 bg-bg-2 font-sans text-3xl font-bold text-indigo-soft">
              {profile?.avatar_url ? (
                <img
                  src={profile.avatar_url}
                  alt={profile.display_name}
                  className="h-full w-full object-cover"
                />
              ) : (
                initial
              )}
            </div>
            <div className="min-w-0 space-y-2">
              <input
                ref={fileInputRef}
                type="file"
                accept="image/png,image/jpeg,image/webp"
                disabled={avatarPending}
                className="hidden"
                onChange={pickAvatar}
              />
              <div className="flex flex-wrap gap-2">
                <ChromeButton
                  type="button"
                  disabled={avatarPending}
                  disabledReason={
                    avatarPending
                      ? t('common:access.operation_pending')
                      : undefined
                  }
                  onClick={() => fileInputRef.current?.click()}
                >
                  {t('account:profile.change_avatar')}
                </ChromeButton>
                {profile?.avatar_url && (
                  <ChromeButton
                    type="button"
                    disabled={avatarPending}
                    disabledReason={
                      avatarPending
                        ? t('common:access.operation_pending')
                        : undefined
                    }
                    onClick={() => removeAvatar.mutate()}
                  >
                    {t('account:profile.remove_avatar')}
                  </ChromeButton>
                )}
              </div>
              <p className="font-sans text-xs leading-relaxed text-tx-3">
                {t('account:profile.avatar_hint')}
              </p>
            </div>
          </div>

          <FormField label={t('account:profile.display_name')} required>
            <FormInput
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
              maxLength={255}
              disabled={save.isPending}
              disabledReason={
                save.isPending
                  ? t('common:access.operation_pending')
                  : undefined
              }
              required
            />
          </FormField>
          <FormField
            label={t('account:profile.email')}
            hint={t('account:profile.email_hint')}
          >
            <FormInput
              type="email"
              value={profile?.email ?? ''}
              readOnly
              aria-readonly="true"
            />
          </FormField>
          <FormField
            label={t('account:profile.bio')}
            hint={t('account:profile.bio_hint')}
          >
            <FormTextarea
              value={bio}
              onChange={(event) => setBio(event.target.value)}
              maxLength={500}
              className="min-h-24"
              disabled={save.isPending}
              disabledReason={
                save.isPending
                  ? t('common:access.operation_pending')
                  : undefined
              }
            />
          </FormField>
          <div className="flex flex-wrap items-center justify-between gap-3 pt-1">
            <span aria-live="polite" className="font-sans text-xs text-tx-3">
              {dirty ? t('settings-admin:preferences.unsaved') : ''}
            </span>
            <div className="flex items-center gap-2">
              <ChromeButton
                type="button"
                disabled={!dirty || save.isPending}
                disabledReason={
                  !dirty
                    ? t('common:access.no_changes')
                    : save.isPending
                      ? t('common:access.operation_pending')
                      : undefined
                }
                onClick={reset}
              >
                {t('common:actions.cancel')}
              </ChromeButton>
              <ChromeButton
                type="submit"
                variant="primary"
                disabled={!dirty || invalid || save.isPending}
                disabledReason={
                  invalid
                    ? t('common:access.form_invalid')
                    : !dirty
                      ? t('common:access.no_changes')
                      : save.isPending
                        ? t('common:access.operation_pending')
                        : undefined
                }
              >
                {save.isPending
                  ? t('common:status.saving')
                  : t('account:profile.save')}
              </ChromeButton>
            </div>
          </div>
        </form>
      )}
    </AccountSection>
  );
}
