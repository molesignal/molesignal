import { useQuery, type UseQueryResult } from '@tanstack/react-query';
import {
  Check,
  Globe2,
  GlobeLock,
  Pencil,
  SlidersHorizontal,
  X,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useParams } from 'react-router-dom';

import * as agentApi from '@/api/agent';
import * as providersApi from '@/api/agent/modelProviders';
import { hasPermission, useProductAccess } from '@/product/access';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { uiSectionTitleClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import { Tabs, TabsContent } from '@/shell/ui/tabs';

import { ImPanel } from './Im';
import { InboundMcpPanel } from './InboundMcp';
import { MemberNetworkPolicy } from './MemberNetwork';
import { ModelsPanel } from './Models';
import { ProfilesPanel } from './Profile';
import {
  ProfileEditorDrawer,
  type ProfileEditorTarget,
} from './Profile/Editor';
import {
  AGENT_SETTINGS_MANAGE,
  AGENT_SETTINGS_READ,
  agentSettingsSection,
  PROFILE_SECTIONS,
} from './sections';
import { AgentSettingsSection } from './Surfaces';
import { UsagePanel } from './Usage';
import { ModulePage } from '../operations/Pages';
import { PromptManagementPanel } from '../prompt/Management';
import { SkillsPanel } from '../skills/SkillsPanel';
import { ToolCapabilitiesPanel } from '../ToolCapabilities';
import { localizeToolMetadataList } from '../toolMetadata';

const EMPTY_TOOLS: agentApi.RegisteredTool[] = [];
const EMPTY_PROVIDERS: providersApi.ModelProvider[] = [];

export function AgentSettingsPage() {
  const { t } = useTranslation('agent');
  const { section } = useParams();
  const access = useProductAccess();
  // Which section is open is chosen in the module's navigation, a row under its tabs.
  const activeTab = agentSettingsSection(section);
  const profilesAccess = useActionAccess({
    permission: 'agent.profiles.manage',
  });
  const toolsAccess = useActionAccess({ permission: 'agent.tools.manage' });
  const managePermission = AGENT_SETTINGS_MANAGE[activeTab];
  const sectionAccess = useActionAccess(
    managePermission ? { permission: managePermission } : {},
  );
  const readPermission = AGENT_SETTINGS_READ[activeTab];
  const readAccess = useActionAccess(
    readPermission ? { permission: readPermission } : {},
  );
  const [profileEditor, setProfileEditor] = React.useState<ProfileEditorTarget>(null);
  // A role can hold one section's permissions without another's, so a section asks only for what it
  // shows, and for what it borrows from another only if the reader may read it.
  const profileSection = PROFILE_SECTIONS.has(activeTab);
  const profiles = useQuery({
    queryKey: ['agent', 'profiles'],
    queryFn: agentApi.listProfiles,
    enabled: profileSection && hasPermission('agent.profiles.read', access),
    retry: false,
  });
  const tools = useQuery({
    queryKey: ['agent', 'tools'],
    queryFn: agentApi.listTools,
    enabled:
      activeTab === 'tools' ||
      (profileSection && hasPermission('agent.tools.read', access)),
    retry: false,
  });
  const providers = useQuery({
    queryKey: ['agent', 'model-providers'],
    queryFn: providersApi.list,
    enabled:
      (activeTab === 'models' || profileSection) &&
      hasPermission('agent.models.read', access),
    retry: false,
  });
  const profileRows = profiles.data ?? [];
  const toolRows = React.useMemo(
    () => localizeToolMetadataList(tools.data?.tools ?? EMPTY_TOOLS, t),
    [t, tools.data?.tools],
  );
  const providerRows = providers.data ?? EMPTY_PROVIDERS;
  const activeProfile =
    profileRows.find((profile) => profile.isDefault) ?? profileRows[0];
  // The notice speaks of what is on the page: the network section may show no profile to its reader,
  // and the models section no provider.
  const sectionLocked =
    managePermission !== undefined &&
    sectionAccess.disabled &&
    (activeTab !== 'network' || activeProfile !== undefined) &&
    (activeTab !== 'models' || hasPermission('agent.models.read', access));
  // The route guard mounts this page once access is known, so this is a refusal and not a wait.
  if (readAccess.disabled) {
    return (
      <ModulePage title={t('settings.title')} bare>
        <ProductState variant="permission-denied" description={readAccess.reason} />
      </ModulePage>
    );
  }
  return (
    <ModulePage title={t('settings.title')} bare>
      <Tabs value={activeTab}>
        {sectionLocked && sectionAccess.reason && (
          <div
            role="status"
            className="mb-3 rounded-md border border-bd-1 bg-bg-2 px-3 py-2 font-sans text-xs text-tx-2"
          >
            {sectionAccess.reason}
          </div>
        )}
        <fieldset
          disabled={profilesAccess.disabled}
          aria-disabled={profilesAccess.disabled || undefined}
          className="contents"
        >
        <TabsContent value="profiles" className="mt-0">
          <ProfilesPanel
            profiles={profiles}
            providers={providerRows}
            onCreate={() =>
              setProfileEditor({ profile: 'new', section: 'profile' })
            }
            onEdit={(profile) =>
              setProfileEditor({ profile, section: 'profile' })
            }
          />
        </TabsContent>
        <TabsContent value="data" className="mt-0">
          {activeProfile ? (
            <DataAccessPanel
              profile={activeProfile}
              onEdit={() =>
                setProfileEditor({
                  profile: activeProfile,
                  section: 'data',
                })
              }
            />
          ) : (
            <NoProfileState profiles={profiles} />
          )}
        </TabsContent>
        <TabsContent value="approvals" className="mt-0">
          {activeProfile ? (
            <ApprovalPolicyPanel
              profile={activeProfile}
              onEdit={() =>
                setProfileEditor({
                  profile: activeProfile,
                  section: 'approvals',
                })
              }
            />
          ) : (
            <NoProfileState profiles={profiles} />
          )}
        </TabsContent>
        </fieldset>
        {/* These answer control by control: what they let a reader see and copy stays open. */}
        <TabsContent value="models" className="mt-0">
          <ModelsPanel providers={providers} />
        </TabsContent>
        <TabsContent value="usage" className="mt-0">
          <UsagePanel />
        </TabsContent>
        <TabsContent value="prompts" className="mt-0">
          <PromptManagementPanel />
        </TabsContent>
        <TabsContent value="inbound-mcp" className="mt-0">
          <InboundMcpPanel />
        </TabsContent>
        <TabsContent value="im" className="mt-0">
          <ImPanel />
        </TabsContent>
        <TabsContent value="network" className="mt-0">
          <NetworkPanel
            profile={activeProfile}
            registry={tools.data}
            editDisabled={profilesAccess.disabled}
            onEdit={
              activeProfile
                ? () =>
                    setProfileEditor({
                      profile: activeProfile,
                      section: 'network',
                    })
                : undefined
            }
          />
        </TabsContent>
        <TabsContent value="skills" className="mt-0">
          <SkillsPanel />
        </TabsContent>
        <TabsContent value="tools" className="mt-0">
          <ToolCapabilitiesPanel registry={tools} manager={!toolsAccess.disabled} />
        </TabsContent>
      </Tabs>
      <ProfileEditorDrawer
        target={profileEditor}
        profiles={profileRows}
        providers={providerRows}
        tools={toolRows}
        onClose={() => setProfileEditor(null)}
      />
    </ModulePage>
  );
}

/** Where a section about a profile has none to speak of: it says why, and shows no defaults instead. */
function NoProfileState({
  profiles,
}: {
  profiles: UseQueryResult<agentApi.AgentProfile[], Error>;
}) {
  const { t } = useTranslation('agent');
  if (profiles.isLoading) return <ProductState variant="loading" />;
  if (profiles.isError) return <ProductState variant="error" error={profiles.error} />;
  return (
    <ProductState
      variant="empty"
      title={t('settings.profiles.empty_title')}
      description={t('settings.profiles.empty_description')}
    />
  );
}

function DataAccessPanel({
  profile,
  onEdit,
}: {
  profile: agentApi.AgentProfile;
  onEdit: () => void;
}) {
  const { t } = useTranslation('agent');
  return (
    <AgentSettingsSection
      title={t('settings.data.title')}
      description={t('settings.data.description')}
      action={
        <EditSectionButton
          label={t('settings.data.configure')}
          onClick={onEdit}
        />
      }
    >
      <ProfileContextLine profile={profile} />
      <div className="grid gap-3 lg:grid-cols-2">
        <PolicyRow label={t('settings.data.environments')} value={scopeValue(profile.dataScope.environments)} />
        <PolicyRow label={t('settings.data.services')} value={scopeValue(profile.dataScope.services, t('settings.data.all_authorized'))} />
        <PolicyRow label={t('settings.data.streams')} value={scopeValue(profile.dataScope.streams, t('settings.data.all_authorized'))} />
        <PolicyRow label={t('settings.data.cross_org')} value={t('settings.blocked')} secure />
      </div>
    </AgentSettingsSection>
  );
}

/**
 * Part of this section is a profile's, part the instance's and part its own, each on a permission of
 * its own: it shows what the reader may read, and says nothing of what it does not know.
 */
function NetworkPanel({
  profile,
  registry,
  editDisabled,
  onEdit,
}: {
  profile: agentApi.AgentProfile | undefined;
  registry: agentApi.ToolRegistry | undefined;
  editDisabled: boolean;
  onEdit: (() => void) | undefined;
}) {
  const { t } = useTranslation('agent');
  const networkAllowed = profile?.networkAccess === 'allowed';
  const controls = [
    { key: 'settings.network.dynamic_http', blocked: registry?.dynamicHttp !== true },
    { key: 'settings.network.shell', blocked: registry?.shell !== true },
    { key: 'settings.network.browser', blocked: registry?.browser !== true },
    { key: 'settings.network.open_mcp', blocked: registry?.openMcp !== true },
  ];
  return (
    <AgentSettingsSection
      title={t('settings.network.title')}
      description={t('settings.network.description')}
      action={
        <EditSectionButton
          label={t('settings.network.configure')}
          disabled={editDisabled}
          onClick={onEdit}
        />
      }
    >
      <ProfileContextLine profile={profile} />
      {profile && (
        <div
          className={cn(
            'rounded-lg border p-4',
            networkAllowed
              ? 'border-yellow/30 bg-yellow-dim'
              : 'border-green/25 bg-green/5',
          )}
        >
          <div className="flex items-center gap-3">
            <div
              className={cn(
                'grid h-9 w-9 place-items-center rounded-md border',
                networkAllowed
                  ? 'border-yellow/30 bg-yellow/10'
                  : 'border-green/30 bg-green/10',
              )}
            >
              {networkAllowed ? (
                <Globe2 className="h-4 w-4 text-[var(--yellow-ink)]" />
              ) : (
                <GlobeLock className="h-4 w-4 text-[var(--green-ink)]" />
              )}
            </div>
            <div>
              <div className="font-strong text-tx-0">{t('settings.network.access')}</div>
              <div
                className={cn(
                  'text-xs',
                  networkAllowed ? 'text-[var(--yellow-ink)]' : 'text-[var(--green-ink)]',
                )}
              >
                {t(networkAllowed ? 'settings.allowed' : 'settings.blocked')}
              </div>
            </div>
          </div>
          <p className="mt-3 max-w-3xl text-sm leading-6 text-tx-2">
            {t(
              networkAllowed
                ? 'settings.network.allowed_explanation'
                : 'settings.network.blocked_explanation',
            )}
          </p>
        </div>
      )}
      {registry && (
        <>
          <div className="mt-4 first:mt-0">
            <h3 className={uiSectionTitleClass}>
              {t('settings.network.capabilities')}
            </h3>
            <p className="mt-1 text-xs leading-5 text-tx-3">
              {t('settings.network.capabilities_description')}
            </p>
          </div>
          <div className="mt-3 grid gap-2 lg:grid-cols-2">
            {controls.map((control) => (
              <div key={control.key} className="flex items-center gap-3 rounded-md border border-bd-0 bg-bg-2 px-3 py-2.5">
                {control.blocked ? <Check className="h-4 w-4 text-[var(--green-ink)]" /> : <X className="h-4 w-4 text-red-soft" />}
                <span className="text-sm text-tx-1">{t(control.key)}</span>
                <Badge variant="outline" className="ml-auto">{control.blocked ? t('settings.blocked') : t('settings.allowed')}</Badge>
              </div>
            ))}
          </div>
        </>
      )}
      <MemberNetworkPolicy />
    </AgentSettingsSection>
  );
}

function ApprovalPolicyPanel({
  profile,
  onEdit,
}: {
  profile: agentApi.AgentProfile;
  onEdit: () => void;
}) {
  const { t } = useTranslation('agent');
  const rows = [
    ['l0', 'settings.approval_policy.automatic'],
    ['l1', 'settings.approval_policy.configurable'],
    ['l2', 'settings.approval_policy.single'],
    ['l3', 'settings.approval_policy.two_person'],
  ] as const;
  return (
    <AgentSettingsSection
      title={t('settings.approval_policy.title')}
      description={t('settings.approval_policy.description')}
      action={
        <EditSectionButton
          label={t('settings.approval_policy.configure')}
          onClick={onEdit}
        />
      }
    >
      <ProfileContextLine profile={profile} />
      <div className="divide-y divide-bd-0 overflow-hidden rounded-lg border border-bd-0 bg-bg-2">
        {rows.map(([risk, fallback]) => {
          const policy = String(profile.riskPolicy[risk] ?? (
            risk === 'l0' || risk === 'l1' ? 'automatic' : risk === 'l2' ? 'approval' : 'two_person_approval'
          ));
          return (
            <div key={risk} className="flex items-center gap-4 px-4 py-3">
              <Badge variant="outline" className="font-mono uppercase">{risk}</Badge>
              <div className="min-w-0 flex-1">
                <div className="font-strong text-tx-0">{t(`settings.approval_policy.${risk}`)}</div>
                <div className="mt-0.5 text-xs text-tx-3">{t(fallback)}</div>
              </div>
              <Badge variant="secondary">
                {t(`settings.profiles.policies.${policy}`, { defaultValue: policy })}
              </Badge>
            </div>
          );
        })}
      </div>
    </AgentSettingsSection>
  );
}

function EditSectionButton({
  label,
  disabled = false,
  onClick,
}: {
  label: string;
  disabled?: boolean;
  onClick: (() => void) | undefined;
}) {
  if (!onClick) return null;
  return (
    <Button size="sm" variant="outline" disabled={disabled} onClick={onClick}>
      <Pencil /> {label}
    </Button>
  );
}

function ProfileContextLine({
  profile,
}: {
  profile: agentApi.AgentProfile | undefined;
}) {
  const { t } = useTranslation('agent');
  if (!profile) return null;
  return (
    <p className="mb-4 border-b border-bd-0 pb-3 text-xs leading-5 text-tx-2">
      {t('settings.profiles.shortcut_hint', { name: profile.name })}
    </p>
  );
}

function PolicyRow({ label, value, secure = false }: { label: string; value: string; secure?: boolean }) {
  return (
    <div className="flex items-center gap-3 rounded-md border border-bd-0 bg-bg-2 p-3">
      <SlidersHorizontal className={cn('h-4 w-4 text-tx-3', secure && 'text-[var(--green-ink)]')} />
      <div>
        <div className="text-xs text-tx-3">{label}</div>
        <div className="mt-0.5 text-sm font-strong text-tx-1">{value}</div>
      </div>
    </div>
  );
}

function scopeValue(value: unknown, fallback = '—'): string {
  if (Array.isArray(value)) return value.length ? value.join(', ') : fallback;
  if (typeof value === 'string' && value) return value;
  return fallback;
}
