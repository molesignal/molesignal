import { Globe } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { uiLabelClass } from '@/shell/chrome';
import { FieldArray, FormInput, FormSection } from '@/shell/FormDrawer';
import { Alert, AlertDescription, AlertTitle } from '@/shell/ui/alert';

import type { ProviderDraft } from '../model';

/**
 * Who may sign in through the provider: the groups allowed to, and, for a source anyone
 * can sign up to, the organization's email domains that keep strangers out.
 */
export function AccessSection({
  draft,
  groupsAvailable,
  publicAccounts,
  emailDomains,
  onChange,
}: {
  draft: ProviderDraft;
  /** False when the platform sends no groups. */
  groupsAvailable: boolean;
  /** The provider takes accounts anyone can create (Google, github.com, gitlab.com). */
  publicAccounts: boolean;
  /** The organization's email domains; undefined while they load. */
  emailDomains: string[] | undefined;
  onChange: (draft: ProviderDraft) => void;
}) {
  const { t } = useTranslation('settings-admin');
  if (!groupsAvailable && !publicAccounts) return null;

  const allowedGroups = draft.allowedGroups.filter((group) => group.trim());
  const domains = emailDomains ?? [];
  const restricted = domains.length > 0 || allowedGroups.length > 0;
  const groupPlaceholder =
    draft.platform === 'github'
      ? t('sso_providers.access.github_group_placeholder')
      : t('sso_providers.access.group_placeholder');

  return (
    <FormSection title={t('sso_providers.access.title')}>
      {publicAccounts && emailDomains !== undefined && (
        <Alert variant={restricted ? 'default' : 'warning'} data-testid="sso-public-accounts">
          <Globe aria-hidden />
          <AlertTitle>{t('sso_providers.access.public_title')}</AlertTitle>
          <AlertDescription>
            {domains.length > 0
              ? t('sso_providers.access.public_domains', {
                  domains: domains.join(t('sso_providers.access.list_separator')),
                })
              : restricted
                ? t('sso_providers.access.public_groups')
                : t('sso_providers.access.public_missing')}{' '}
            <Link to="/iam/email-domains" className="text-[var(--blue-ink)] hover:underline">
              {t('sso_providers.access.email_domains_link')}
            </Link>
          </AlertDescription>
        </Alert>
      )}
      {groupsAvailable && (
        <div className="flex min-w-0 flex-col gap-1.5">
          <span className={uiLabelClass}>{t('sso_providers.access.allowed_groups')}</span>
          <span className="text-xs leading-relaxed text-tx-3">
            {t('sso_providers.access.allowed_groups_hint')}
          </span>
          <FieldArray
            items={draft.allowedGroups}
            onChange={(allowedGroups) => onChange({ ...draft, allowedGroups })}
            newItem={() => ''}
            addLabel={t('sso_providers.access.add_group')}
            removeLabel={t('sso_providers.access.remove_group')}
            renderItem={(group, _index, setGroup) => (
              <FormInput
                value={group}
                onChange={(event) => setGroup(event.target.value)}
                placeholder={groupPlaceholder}
                aria-label={t('sso_providers.access.allowed_groups')}
                spellCheck={false}
              />
            )}
          />
        </div>
      )}
    </FormSection>
  );
}

/** Saving is held back while a public-account provider is open to anyone. */
export function publicAccountsUnrestricted(
  draft: ProviderDraft,
  publicAccounts: boolean,
  emailDomains: string[] | undefined,
): boolean {
  if (!publicAccounts || emailDomains === undefined) return false;
  return emailDomains.length === 0 && !draft.allowedGroups.some((group) => group.trim());
}
