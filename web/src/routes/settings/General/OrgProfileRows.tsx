import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as orgsApi from '@/api/orgs';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';
import { OrgMark } from '@/shell/OrgSwitcher';
import { toast } from '@/shell/ui/sonner';
import { useCurrentOrgSelection, useOrgStore } from '@/stores/useOrgStore';

import { SettingsDraftStatus, SettingsRow } from '../_atoms';
import { LOGO_ACCEPT, LogoImageError, toLogoDataUrl } from './logoImage';

const PROFILE_QUERY_KEY = ['workspace', 'profile'] as const;

/**
 * The organization's logo and description, as the organization switcher in the top bar shows them.
 * A logo is kept as soon as it is chosen; the description when the field is left or Enter pressed.
 */
export function OrgProfileRows({ access }: { access: ActionAccess }) {
  const { t } = useTranslation(['settings-admin', 'common']);
  const qc = useQueryClient();
  const productAccess = useProductAccess();
  const { currentOrgId, orgOptions } = useCurrentOrgSelection();
  const upsertOrg = useOrgStore((state) => state.upsertOrg);
  const fileRef = React.useRef<HTMLInputElement>(null);
  const query = useQuery({
    queryKey: PROFILE_QUERY_KEY,
    queryFn: orgsApi.profile,
    enabled: hasPermission('org.preferences.read', productAccess),
  });
  const saved = query.data;
  const [description, setDescription] = React.useState('');

  React.useEffect(() => {
    if (saved) setDescription(saved.description);
  }, [saved]);

  const save = useMutation({
    mutationFn: orgsApi.updateProfile,
    onSuccess: (profile) => {
      qc.setQueryData(PROFILE_QUERY_KEY, profile);
      setDescription(profile.description);
      // The switcher reads the organization list: it shows the new logo without a reload.
      const current = orgOptions.find((org) => org.id === currentOrgId);
      if (current) upsertOrg({ ...current, logo: profile.logo, description: profile.description || null });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const locked = access.disabled || !saved || save.isPending;
  const descriptionDirty = Boolean(saved) && description.trim() !== saved?.description;
  const saveDescription = () => {
    if (saved && descriptionDirty) save.mutate({ logo: saved.logo, description: description.trim() });
  };
  const chooseLogo = async (file: File | undefined) => {
    if (!file || !saved) return;
    try {
      save.mutate({ logo: await toLogoDataUrl(file), description: saved.description });
    } catch (error) {
      const reason = error instanceof LogoImageError ? error.reason : 'unreadable';
      toast.error(t(`general.organization_logo_${reason}`));
    }
  };

  return (
    <>
      <SettingsRow
        label={t('general.organization_logo')}
        description={t('general.organization_logo_hint')}
        controlClassName="w-full"
      >
        <div className="flex items-center gap-3" data-testid="org-logo-field">
          <OrgMark logo={saved?.logo} size="lg" />
          <input
            ref={fileRef}
            type="file"
            accept={LOGO_ACCEPT}
            className="sr-only"
            tabIndex={-1}
            aria-hidden
            data-testid="org-logo-input"
            onChange={(event) => {
              void chooseLogo(event.target.files?.[0]);
              event.target.value = '';
            }}
          />
          <ChromeButton
            size="sm"
            disabled={locked}
            title={access.reason}
            onClick={() => fileRef.current?.click()}
          >
            {saved?.logo ? t('general.organization_logo_replace') : t('general.organization_logo_upload')}
          </ChromeButton>
          {saved?.logo && (
            <ChromeButton
              size="sm"
              disabled={locked}
              title={access.reason}
              onClick={() => save.mutate({ logo: null, description: saved.description })}
            >
              {t('general.organization_logo_remove')}
            </ChromeButton>
          )}
        </div>
      </SettingsRow>
      <SettingsRow
        label={t('general.organization_description')}
        description={t('general.organization_description_hint')}
        controlClassName="w-full"
      >
        <div className="w-full">
          <FormInput
            value={description}
            maxLength={orgsApi.ORG_DESCRIPTION_MAX_CHARS}
            placeholder={t('general.organization_description_placeholder')}
            onChange={(event) => {
              save.reset();
              setDescription(event.target.value);
            }}
            onBlur={saveDescription}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault();
                saveDescription();
              }
              if (event.key === 'Escape') setDescription(saved?.description ?? '');
            }}
            aria-label={t('general.organization_description')}
            disabled={access.disabled || !saved}
            disabledReason={access.reason}
            className="h-11 text-base lg:h-9 lg:text-sm"
          />
          <div className="mt-2">
            <SettingsDraftStatus
              dirty={descriptionDirty || save.isPending}
              error={save.isError}
              modifiedLabel={t('general.auto_save.modified')}
              undoLabel={t('general.auto_save.undo')}
              errorLabel={t('general.auto_save.error')}
              retryLabel={t('general.auto_save.retry')}
              onUndo={() => setDescription(saved?.description ?? '')}
              onRetry={saveDescription}
            />
          </div>
        </div>
      </SettingsRow>
    </>
  );
}
