import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { PageHeader } from '@/admin';
import * as dashboardsApi from '@/api/dashboards';
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
import { useOrgStore } from '@/stores/useOrgStore';

import {
  SectionBody,
  SettingsGroupStack,
  SettingsSection,
} from '../_atoms';
import { DangerZone } from './DangerZone';
import {
  PreferenceDefaultsSection,
  SharingPolicySection,
} from './sections';
import { useAutoSave } from './useAutoSave';
import { WorkspaceSection } from './WorkspaceSection';
import { useSettingsSaveStatus } from '../SettingsSaveStatus';

const SHARE_POLICY_FIELDS = [
  'allowPublicLinks',
  'allowPublicDashboards',
  'maxPublicExpirySecs',
  'requirePublicReportPassword',
  'denyProductionPublicShares',
  'allowPublicCsvDownload',
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
  const access = useProductAccess();
  const canReadSettings = hasPermission('org.preferences.read', access);
  const manageAccess = useActionAccess({
    permission: 'org.preferences.manage',
  });
  const canManageSettings = manageAccess.allowed;
  const canReadSharing = hasPermission('org.sharing.read', access);
  const shareAccess = useActionAccess({
    permission: 'org.sharing.manage',
  });
  const [workspaceName, setWorkspaceName] = React.useState('');
  const [sharePolicyDraft, setSharePolicyDraft] =
    React.useState<resourceSharesApi.ResourceSharePolicy | null>(null);
  const [preferenceDefaults, setPreferenceDefaults] =
    React.useState<meApi.UserPreferences>(meApi.DEFAULT_USER_PREFERENCES);
  const [preferenceDefaultsBaseline, setPreferenceDefaultsBaseline] =
    React.useState<meApi.UserPreferences>(meApi.DEFAULT_USER_PREFERENCES);

  React.useEffect(() => {
    if (profile) setWorkspaceName(profile.orgName);
  }, [profile]);

  React.useEffect(() => {
    saveStatus.setDraftDirty(
      'general.workspace_name',
      Boolean(profile && workspaceName.trim() !== profile.orgName),
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
      return orgsApi.updateOrg(profile.orgId, { name });
    },
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (organization) => {
      upsertOrg(organization);
      setWorkspaceName(organization.name);
      qc.setQueryData<meApi.MeProfile>(['me', 'profile'], (current) =>
        current
          ? {
              ...current,
              orgName: organization.name,
              orgSlug: organization.slug ?? current.orgSlug,
            }
          : current,
      );
      saveStatus.completeSave();
    },
    onError: (error) => {
      saveStatus.failSave();
      toast.error(toApiError(error).message);
    },
  });

  const sharePolicyQuery = useQuery({
    queryKey: ['resource-shares', 'policy', profile?.orgId],
    queryFn: resourceSharesApi.getPolicy,
    enabled: canReadSharing,
  });
  const sharePolicy = sharePolicyDraft ?? sharePolicyQuery.data;
  const preferenceDefaultsQuery = useQuery({
    queryKey: ['workspace', 'preference-defaults', profile?.orgId],
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
    profile && workspaceName.trim() !== profile.orgName,
  );
  const workspaceNameInvalid = workspaceName.trim().length === 0;
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
    if (sharePolicyQuery.data) setSharePolicyDraft(sharePolicyQuery.data);
  }, [sharePolicyQuery.data]);

  React.useEffect(() => {
    saveStatus.setDraftDirty(
      'general.workspace_preference_defaults',
      preferenceDefaultsDirty,
    );
  }, [preferenceDefaultsDirty, saveStatus]);

  React.useEffect(() => {
    saveStatus.setDraftDirty('general.sharing_policy', sharePolicyDirty);
  }, [saveStatus, sharePolicyDirty]);

  React.useEffect(
    () => () => {
      saveStatus.setDraftDirty('general.workspace_name', false);
    },
    [saveStatus],
  );

  const updateSharePolicy = useMutation({
    mutationFn: resourceSharesApi.updatePolicy,
    onMutate: () => {
      saveStatus.beginSave();
    },
    onSuccess: (saved) => {
      qc.setQueryData(
        ['resource-shares', 'policy', profile?.orgId],
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
        ['workspace', 'preference-defaults', profile?.orgId],
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

  const setSharePolicy = (
    patch: Partial<
      Omit<
        resourceSharesApi.ResourceSharePolicy,
        'organizationId' | 'updatedBy' | 'updatedAt'
      >
    >,
  ) => {
    if (!shareAccess.allowed || !sharePolicy || updateSharePolicy.isPending) return;
    updateSharePolicy.reset();
    setSharePolicyDraft({
      ...sharePolicy,
      allowPublicLinks: sharePolicy.allowPublicLinks,
      allowPublicDashboards: sharePolicy.allowPublicDashboards,
      maxPublicExpirySecs: sharePolicy.maxPublicExpirySecs,
      requirePublicReportPassword:
        sharePolicy.requirePublicReportPassword,
      denyProductionPublicShares:
        sharePolicy.denyProductionPublicShares,
      allowPublicCsvDownload: sharePolicy.allowPublicCsvDownload,
      ...patch,
    });
  };

  const retryWorkspaceName = useAutoSave({
    fingerprint: workspaceName.trim(),
    enabled:
      canManageSettings &&
      Boolean(profile) &&
      workspaceNameDirty &&
      !workspaceNameInvalid,
    busy: updateWorkspaceName.isPending,
    save: () => updateWorkspaceName.mutateAsync(workspaceName.trim()),
  });

  const retrySharePolicy = useAutoSave({
    fingerprint: sharePolicy
      ? JSON.stringify(SHARE_POLICY_FIELDS.map((field) => sharePolicy[field]))
      : '',
    enabled: shareAccess.allowed && Boolean(sharePolicy) && sharePolicyDirty,
    busy: updateSharePolicy.isPending,
    save: () => {
      if (!sharePolicy) return Promise.resolve();
      return updateSharePolicy.mutateAsync({
        allowPublicLinks: sharePolicy.allowPublicLinks,
        allowPublicDashboards: sharePolicy.allowPublicDashboards,
        maxPublicExpirySecs: sharePolicy.maxPublicExpirySecs,
        requirePublicReportPassword:
          sharePolicy.requirePublicReportPassword,
        denyProductionPublicShares:
          sharePolicy.denyProductionPublicShares,
        allowPublicCsvDownload: sharePolicy.allowPublicCsvDownload,
      });
    },
  });

  const retryPreferenceDefaults = useAutoSave({
    fingerprint: JSON.stringify(preferenceDefaults),
    enabled: canManageSettings && preferenceDefaultsDirty,
    busy: updatePreferenceDefaults.isPending,
    save: () => updatePreferenceDefaults.mutateAsync(),
  });

  const resetPreferenceDefaults = () => {
    setPreferenceDefaults(preferenceDefaultsBaseline);
  };

  React.useEffect(
    () => () => {
      saveStatus.setDraftDirty(
        'general.workspace_preference_defaults',
        false,
      );
      saveStatus.setDraftDirty('general.sharing_policy', false);
    },
    [saveStatus],
  );

  return (
    <>
      <PageHeader
        title={t('general.title')}
        subtitle={t('general.subtitle')}
        className="min-h-0 border-b-0 bg-transparent px-0 py-0"
      />
      <SectionBody opensOnSection className="pb-10">
        <SettingsGroupStack>
          <WorkspaceSection
            profile={profile}
            state={pageState}
            name={workspaceName}
            dirty={workspaceNameDirty}
            invalid={workspaceNameInvalid}
            access={manageAccess}
            pending={updateWorkspaceName.isPending}
            error={updateWorkspaceName.isError}
            onNameChange={(name) => {
              updateWorkspaceName.reset();
              setWorkspaceName(name);
            }}
            onReset={() => {
              updateWorkspaceName.reset();
              setWorkspaceName(profile?.orgName ?? '');
            }}
            onSave={retryWorkspaceName}
          />

          {canReadSharing ? (
            <SettingsSection
              title={t('general.access_control.title')}
              description={t('general.access_control.subtitle')}
              contentClassName="gap-0"
            >
              <SharingPolicySection
                policy={sharePolicy}
                isLoading={sharePolicyQuery.isLoading}
                isError={sharePolicyQuery.isError}
                error={sharePolicyQuery.error}
                pending={updateSharePolicy.isPending}
                saveError={updateSharePolicy.isError}
                dirty={sharePolicyDirty}
                canManage={shareAccess.allowed}
                disabledReason={shareAccess.reason}
                onChange={setSharePolicy}
                onReset={() => {
                  updateSharePolicy.reset();
                  setSharePolicyDraft(sharePolicyQuery.data ?? null);
                }}
                onSave={retrySharePolicy}
              />
            </SettingsSection>
          ) : null}

          {canReadSettings ? (
            <PreferenceDefaultsSection
              value={preferenceDefaults}
              dashboards={dashboardsQuery.data ?? []}
              dirty={preferenceDefaultsDirty}
              isLoading={preferenceDefaultsQuery.isLoading}
              isError={preferenceDefaultsQuery.isError}
              error={preferenceDefaultsQuery.error}
              pending={updatePreferenceDefaults.isPending}
              saveError={updatePreferenceDefaults.isError}
              canManage={canManageSettings}
              disabledReason={manageAccess.reason}
              onChange={(patch) => {
                updatePreferenceDefaults.reset();
                setPreferenceDefaults((current) => ({
                  ...current,
                  ...patch,
                }));
              }}
              onReset={() => {
                updatePreferenceDefaults.reset();
                resetPreferenceDefaults();
              }}
              onSave={retryPreferenceDefaults}
            />
          ) : null}

          <DangerZone />
        </SettingsGroupStack>
      </SectionBody>
    </>
  );
}
