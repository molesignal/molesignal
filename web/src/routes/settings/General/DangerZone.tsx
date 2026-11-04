import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import { removeOrg } from '@/api/orgs';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import { ChromeButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';
import { signOut } from '@/stores/auth/signOut';

import { usePlatformAction } from './platformAction';
import { SettingsRow, SettingsSection } from '../_atoms';

/**
 * Deleting an organization takes one of two permissions: `org.delete`, which deletes this one from
 * inside it, or the platform's management of organizations, which is reached in the platform
 * organization. Anyone with neither is told what it takes.
 */
export function DangerZone() {
  const { t } = useTranslation(['settings-admin', 'common']);
  const navigate = useNavigate();
  const access = useProductAccess();
  const management = usePlatformAction('sys.organizations.delete');
  const deletesItself = access?.scope === 'organization' && hasPermission('org.delete', access);

  return (
    <SettingsSection
      title={t('general.danger.title')}
      description={t('general.danger.subtitle')}
      tone="danger"
    >
      <SettingsRow
        label={t('general.danger.delete_title')}
        description={t(deletesItself ? 'general.danger.delete_own_description' : 'general.danger.delete_description')}
      >
        {deletesItself ? (
          <DeleteOwnOrganization />
        ) : management.reach === 'none' ? (
          <p className="m-0 max-w-[320px] font-sans text-sm leading-relaxed text-tx-2">
            {t('general.danger.platform_only')}
          </p>
        ) : (
          <div className="flex max-w-[360px] flex-col items-start gap-1.5">
            <ChromeButton
              variant="danger"
              disabled={management.reach === 'loading' || management.entering}
              disabledReason={management.reach === 'loading' ? t('common:access.loading') : undefined}
              onClick={() => void management.run(() => navigate('/settings/organization_management'))}
              className="h-11 text-base lg:h-9 lg:text-sm"
            >
              {t('general.danger.manage_delete')}
            </ChromeButton>
            {management.reach === 'platform' && (
              <span className="font-sans text-xs leading-relaxed text-tx-3">
                {t('general.danger.enters_platform')}
              </span>
            )}
          </div>
        )}
      </SettingsRow>
    </SettingsSection>
  );
}

/** The session belongs to the organization, so it ends with it: the next sign-in lands elsewhere. */
function DeleteOwnOrganization() {
  const { t } = useTranslation(['settings-admin', 'common']);
  const navigate = useNavigate();
  const organization = useAuthStore((state) => state.ctx);
  const [confirming, setConfirming] = React.useState(false);
  const name = organization?.orgName ?? organization?.orgId ?? '';

  const remove = useMutation({
    mutationFn: () => removeOrg(organization?.orgId ?? ''),
    onSuccess: async () => {
      toast.success(t('general.danger.deleted', { name }));
      await signOut();
      navigate('/signin');
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <>
      <ChromeButton
        variant="danger"
        onClick={() => setConfirming(true)}
        className="h-11 text-base lg:h-9 lg:text-sm"
      >
        {t('general.danger.delete_title')}
      </ChromeButton>
      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        destructive
        title={t('general.danger.delete_confirm_title', { name })}
        description={t('general.danger.delete_confirm_description')}
        typeToConfirm={name}
        confirmLabel={t('common:actions.delete')}
        busy={remove.isPending}
        onConfirm={() => remove.mutate()}
      />
    </>
  );
}
