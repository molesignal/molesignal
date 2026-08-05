import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { PageHeader } from '@/admin';
import * as dashboardsApi from '@/api/dashboards';
import * as instanceApi from '@/api/instance';
import * as meApi from '@/api/me';
import * as orgsApi from '@/api/orgs';
import * as resourceSharesApi from '@/api/resourceShares';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { USER_PREFERENCES_QUERY_KEY } from '@/shell/PreferenceRuntime';
import {
  normalizeUserPreferences,
  userPreferencesEqual,
} from '@/shell/PreferencesFields';
import { toast } from '@/shell/ui/sonner';
import { normalizeRole } from '@/stores/auth';
import { useOrgStore } from '@/stores/useOrgStore';

import { SectionBody, SettingsGroupStack } from '../_atoms';
import {
  PreferenceDefaultsSection,
  SharingPolicySection,
  SignupPolicySection,
} from './sections';
import { WorkspaceSection } from './WorkspaceSection';
import { useSettingsSaveStatus } from '../SettingsSaveStatus';

const SHARE_POLICY_FIELDS = [
  'allow_public_links',
  'allow_public_dashboards',
  'max_public_expiry_secs',
  'require_public_report_password',
  'deny_production_public_shares',
  'allow_public_csv_download',
] as const satisfies readonly (keyof resourceSharesApi.ResourceSharePolicy)[];

export function General() {
  const { t } = useTranslation(['settings-admin', 'common']);
  const qc = useQueryClient();
  const saveStatus = useSettingsSaveStatus();
  const upsertOrg = useOrgStore((state) => state.upsertOrg);
  const profileQuery = useQuery({
    queryKey: ['me', 'profile'],
    queryFn: () => meApi.profile(),
  });
  const profile = profileQuery.data;
  const role = profile ? normalizeRole(profile.display_role) : '—';
  const access = useProductAccess();
  const canReadSettings = hasPermission('org.settings.read', access);
  const manageAccess = useActionAccess({
    permission: 'org.settings.manage',
  });
  const canManageSettings = manageAccess.allowed;
  const [workspaceName, setWorkspaceName] = React.useState('');
  const [signupPolicyDraft, setSignupPolicyDraft] =
    React.useState<instanceApi.SignupPolicy | null>(null);
  const [sharePolicyDraft, setSharePolicyDraft] =
    React.useState<resourceSharesApi.ResourceSharePolicy | null>(null);
  const [preferenceDefaults, setPreferenceDefaults] =
    React.useState<meApi.UserPreferences>(meApi.DEFAULT_USER_PREFERENCES);
  const [preferenceDefaultsBaseline, setPreferenceDefaultsBaseline] =
    React.useState<meApi.UserPreferences>(meApi.DEFAULT_USER_PREFERENCES);

  React.useEffect(() => {
    if (profile) setWorkspaceName(profile.org_name);
  }, [profile]);

  React.useEffect(() => {
    saveStatus.setDraftDirty(
      'general.workspace_name',
      Boolean(profile && workspaceName.trim() !== profile.org_name),
    );
  }, [profile, saveStatus, workspaceName]);

  const pageState = productStateFor(
    profileQuery.isLoading ? 'loading' : profileQuery.isError ? 'error' : null,
    {
      error: profileQuery.error,
      emptyTitle: t('general.title'),
      emptyDescription: t('general.empty_description'),
    },
  );

  const updateWorkspaceName = useMutation({
    mutationFn: (name: string) => {
      if (!profile) throw new Error('missing workspace profile');
      return orgsApi.updateOrg(profile.org_id, { name });
    },
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (organization) => {
      upsertOrg(organization);
      setWorkspaceName(organization.name);
      qc.setQueryData<meApi.MeProfile>(['me', 'profile'], (current) =>
        current
          ? {
              ...current,
              org_name: organization.name,
              org_slug: organization.slug ?? current.org_slug,
            }
          : current,
      );
      saveStatus.completeSave();
    },
    onError: (error) => {
      setWorkspaceName(profile?.org_name ?? '');
      saveStatus.failSave();
      toast.error(toApiError(error).message);
    },
  });

  const saveWorkspaceName = React.useCallback(() => {
    if (!canManageSettings || !profile || updateWorkspaceName.isPending) return;
    const next = workspaceName.trim();
    if (!next) {
      setWorkspaceName(profile.org_name);
      return;
    }
    if (next !== profile.org_name) updateWorkspaceName.mutate(next);
  }, [canManageSettings, profile, updateWorkspaceName, workspaceName]);

  const policyQuery = useQuery({
    queryKey: ['settings', 'signup'],
    queryFn: () => instanceApi.getSignupPolicy(),
    enabled: canReadSettings,
  });
  const policy = signupPolicyDraft ?? policyQuery.data;
  const sharePolicyQuery = useQuery({
    queryKey: ['resource-shares', 'policy', profile?.org_id],
    queryFn: resourceSharesApi.getPolicy,
    enabled: canReadSettings,
  });
  const sharePolicy = sharePolicyDraft ?? sharePolicyQuery.data;
  const preferenceDefaultsQuery = useQuery({
    queryKey: ['workspace', 'preference-defaults', profile?.org_id],
    queryFn: () => orgsApi.preferenceDefaults(),
    enabled: canReadSettings,
  });
  const dashboardsQuery = useQuery({
    queryKey: ['dashboards'],
    queryFn: () => dashboardsApi.list(),
    enabled: canReadSettings && hasPermission('dashboards.read', access),
    staleTime: 60_000,
  });
  const preferenceDefaultsDirty = !userPreferencesEqual(
    preferenceDefaults,
    preferenceDefaultsBaseline,
  );
  const workspaceNameDirty = Boolean(
    profile && workspaceName.trim() !== profile.org_name,
  );
  const workspaceNameInvalid = workspaceName.trim().length === 0;
  const signupPolicyDirty = Boolean(
    policyQuery.data &&
      policy &&
      (policy.signup_enabled !== policyQuery.data.signup_enabled ||
        policy.signup_require_approval !==
          policyQuery.data.signup_require_approval),
  );
  const sharePolicyDirty = Boolean(
    sharePolicyQuery.data &&
      sharePolicy &&
      SHARE_POLICY_FIELDS.some(
        (field) => sharePolicy[field] !== sharePolicyQuery.data?.[field],
      ),
  );

  React.useEffect(() => {
    if (!preferenceDefaultsQuery.data) return;
    const normalized = normalizeUserPreferences(
      preferenceDefaultsQuery.data,
    );
    setPreferenceDefaults(normalized);
    setPreferenceDefaultsBaseline(normalized);
  }, [preferenceDefaultsQuery.data]);

  React.useEffect(() => {
    if (policyQuery.data) setSignupPolicyDraft(policyQuery.data);
  }, [policyQuery.data]);

  React.useEffect(() => {
    if (sharePolicyQuery.data) setSharePolicyDraft(sharePolicyQuery.data);
  }, [sharePolicyQuery.data]);

  React.useEffect(() => {
    saveStatus.setDraftDirty(
      'general.workspace_preference_defaults',
      preferenceDefaultsDirty,
    );
  }, [preferenceDefaultsDirty, saveStatus]);

  React.useEffect(() => {
    saveStatus.setDraftDirty('general.signup_policy', signupPolicyDirty);
  }, [saveStatus, signupPolicyDirty]);

  React.useEffect(() => {
    saveStatus.setDraftDirty('general.sharing_policy', sharePolicyDirty);
  }, [saveStatus, sharePolicyDirty]);

  React.useEffect(
    () => () => {
      saveStatus.setDraftDirty('general.workspace_name', false);
    },
    [saveStatus],
  );

  const updatePolicy = useMutation({
    mutationFn: (next: instanceApi.SignupPolicy) => instanceApi.updateSignupPolicy(next),
    onMutate: () => {
      saveStatus.beginSave();
    },
    onSuccess: (saved) => {
      qc.setQueryData(['settings', 'signup'], saved);
      setSignupPolicyDraft(saved);
      saveStatus.completeSave();
    },
    onError: (error) => {
      saveStatus.failSave();
      toast.error(toApiError(error).message);
    },
  });
  const updateSharePolicy = useMutation({
    mutationFn: resourceSharesApi.updatePolicy,
    onMutate: () => {
      saveStatus.beginSave();
    },
    onSuccess: (saved) => {
      qc.setQueryData(
        ['resource-shares', 'policy', profile?.org_id],
        saved,
      );
      setSharePolicyDraft(saved);
      saveStatus.completeSave();
    },
    onError: (error) => {
      saveStatus.failSave();
      toast.error(toApiError(error).message);
    },
  });
  const updatePreferenceDefaults = useMutation({
    mutationFn: () => orgsApi.updatePreferenceDefaults(preferenceDefaults),
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (saved) => {
      const normalized = normalizeUserPreferences(saved);
      setPreferenceDefaults(normalized);
      setPreferenceDefaultsBaseline(normalized);
      qc.setQueryData(
        ['workspace', 'preference-defaults', profile?.org_id],
        normalized,
      );
      void qc.invalidateQueries({ queryKey: USER_PREFERENCES_QUERY_KEY });
      saveStatus.completeSave();
    },
    onError: (error) => {
      saveStatus.failSave();
      toast.error(toApiError(error).message);
    },
  });

  const setPolicy = (patch: Partial<instanceApi.SignupPolicy>) => {
    if (!canManageSettings || !policy || updatePolicy.isPending) return;
    setSignupPolicyDraft({ ...policy, ...patch });
  };
  const setSharePolicy = (
    patch: Partial<
      Omit<
        resourceSharesApi.ResourceSharePolicy,
        'organization_id' | 'updated_by' | 'updated_at'
      >
    >,
  ) => {
    if (!canManageSettings || !sharePolicy || updateSharePolicy.isPending) return;
    setSharePolicyDraft({
      ...sharePolicy,
      allow_public_links: sharePolicy.allow_public_links,
      allow_public_dashboards: sharePolicy.allow_public_dashboards,
      max_public_expiry_secs: sharePolicy.max_public_expiry_secs,
      require_public_report_password:
        sharePolicy.require_public_report_password,
      deny_production_public_shares:
        sharePolicy.deny_production_public_shares,
      allow_public_csv_download: sharePolicy.allow_public_csv_download,
      ...patch,
    });
  };

  const saveSignupPolicy = () => {
    if (
      !canManageSettings ||
      !policy ||
      !signupPolicyDirty ||
      updatePolicy.isPending
    ) {
      return;
    }
    updatePolicy.mutate(policy);
  };

  const saveSharePolicy = () => {
    if (
      !canManageSettings ||
      !sharePolicy ||
      !sharePolicyDirty ||
      updateSharePolicy.isPending
    ) {
      return;
    }
    updateSharePolicy.mutate({
      allow_public_links: sharePolicy.allow_public_links,
      allow_public_dashboards: sharePolicy.allow_public_dashboards,
      max_public_expiry_secs: sharePolicy.max_public_expiry_secs,
      require_public_report_password:
        sharePolicy.require_public_report_password,
      deny_production_public_shares:
        sharePolicy.deny_production_public_shares,
      allow_public_csv_download: sharePolicy.allow_public_csv_download,
    });
  };

  const resetPreferenceDefaults = () => {
    setPreferenceDefaults(preferenceDefaultsBaseline);
  };

  React.useEffect(
    () => () => {
      saveStatus.setDraftDirty(
        'general.workspace_preference_defaults',
        false,
      );
      saveStatus.setDraftDirty('general.signup_policy', false);
      saveStatus.setDraftDirty('general.sharing_policy', false);
    },
    [saveStatus],
  );

  return (
    <>
      <PageHeader title={t('general.title')} subtitle={t('general.subtitle')} />
      <SectionBody className="pb-10">
        <SettingsGroupStack>
          <WorkspaceSection
            profile={profile}
            role={role}
            state={pageState}
            name={workspaceName}
            dirty={workspaceNameDirty}
            invalid={workspaceNameInvalid}
            access={manageAccess}
            pending={updateWorkspaceName.isPending}
            onNameChange={setWorkspaceName}
            onReset={() => setWorkspaceName(profile?.org_name ?? '')}
            onSave={saveWorkspaceName}
          />

          <SignupPolicySection
            policy={policy}
            isLoading={policyQuery.isLoading}
            isError={policyQuery.isError}
            error={policyQuery.error}
            pending={updatePolicy.isPending}
            dirty={signupPolicyDirty}
            canManage={canManageSettings}
            disabledReason={manageAccess.reason}
            onChange={setPolicy}
            onReset={() => setSignupPolicyDraft(policyQuery.data ?? null)}
            onSave={saveSignupPolicy}
          />

          <SharingPolicySection
            policy={sharePolicy}
            isLoading={sharePolicyQuery.isLoading}
            isError={sharePolicyQuery.isError}
            error={sharePolicyQuery.error}
            pending={updateSharePolicy.isPending}
            dirty={sharePolicyDirty}
            canManage={canManageSettings}
            disabledReason={manageAccess.reason}
            onChange={setSharePolicy}
            onReset={() =>
              setSharePolicyDraft(sharePolicyQuery.data ?? null)
            }
            onSave={saveSharePolicy}
          />

          <PreferenceDefaultsSection
            value={preferenceDefaults}
            dashboards={dashboardsQuery.data ?? []}
            dirty={preferenceDefaultsDirty}
            isLoading={preferenceDefaultsQuery.isLoading}
            isError={preferenceDefaultsQuery.isError}
            error={preferenceDefaultsQuery.error}
            pending={updatePreferenceDefaults.isPending}
            canManage={canManageSettings}
            disabledReason={manageAccess.reason}
            onChange={(patch) =>
              setPreferenceDefaults((current) => ({
                ...current,
                ...patch,
              }))
            }
            onReset={resetPreferenceDefaults}
            onSave={() => updatePreferenceDefaults.mutate()}
          />
        </SettingsGroupStack>
      </SectionBody>
    </>
  );
}
