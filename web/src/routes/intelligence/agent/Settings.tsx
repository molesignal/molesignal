import { useQuery, type UseQueryResult } from '@tanstack/react-query';
import {
  Bot,
  BrainCircuit,
  Check,
  Database,
  FileText,
  Globe2,
  GlobeLock,
  KeyRound,
  Pencil,
  Plus,
  ShieldCheck,
  SlidersHorizontal,
  Wrench,
  X,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import * as intelligenceApi from '@/api/intelligence';
import * as providersApi from '@/api/intelligence/modelProviders';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shell/ui/tabs';

import {
  ModelProviderEditorDrawer,
  ProfileEditorDrawer,
  type ProfileEditorTarget,
  type ProviderEditorTarget,
} from './SettingsEditors';
import { ModulePage } from '../operations/Pages';
import { PromptManagementPanel } from '../prompt/Management';
import { ToolCapabilitiesPanel } from '../ToolCapabilities';

const SETTINGS_TABS = [
  { value: 'profiles', key: 'settings.tabs.profiles', icon: Bot },
  { value: 'models', key: 'settings.tabs.models', icon: BrainCircuit },
  { value: 'prompts', key: 'settings.tabs.prompts', icon: FileText },
  { value: 'tools', key: 'settings.tabs.tools', icon: Wrench },
  { value: 'data', key: 'settings.tabs.data', icon: Database },
  { value: 'network', key: 'settings.tabs.network', icon: GlobeLock },
  { value: 'approvals', key: 'settings.tabs.approvals', icon: ShieldCheck },
] as const;

const EMPTY_TOOLS: intelligenceApi.RegisteredTool[] = [];
const EMPTY_PROVIDERS: providersApi.ModelProvider[] = [];

export function AgentSettingsPage() {
  const { t } = useTranslation('intelligence');
  const navigate = useNavigate();
  const { section } = useParams();
  const manageAccess = useActionAccess({
    permission: 'intelligence.manage',
  });
  const [profileEditor, setProfileEditor] = React.useState<ProfileEditorTarget>(null);
  const profiles = useQuery({
    queryKey: ['intelligence', 'profiles'],
    queryFn: intelligenceApi.listProfiles,
    retry: false,
  });
  const tools = useQuery({
    queryKey: ['intelligence', 'tools'],
    queryFn: intelligenceApi.listTools,
    retry: false,
  });
  const providers = useQuery({
    queryKey: ['intelligence', 'model-providers'],
    queryFn: providersApi.list,
    retry: false,
  });
  const profileRows = profiles.data ?? [];
  const toolRows = tools.data?.tools ?? EMPTY_TOOLS;
  const providerRows = providers.data ?? EMPTY_PROVIDERS;
  const activeProfile =
    profileRows.find((profile) => profile.is_default) ?? profileRows[0];
  const activeTab: string =
    section && SETTINGS_TABS.some((tab) => tab.value === section)
      ? section
      : 'profiles';
  return (
    <ModulePage title={t('settings.title')} description={t('settings.description')}>
      <Tabs
        value={activeTab}
        onValueChange={(value) =>
          navigate(
            value === 'profiles'
              ? '/intelligence/settings'
              : `/intelligence/settings/${value}`,
          )
        }
      >
        <TabsList className="max-w-full justify-start overflow-x-auto">
          {SETTINGS_TABS.map((tab) => {
            const Icon = tab.icon;
            return (
              <TabsTrigger key={tab.value} value={tab.value} className="gap-2">
                <Icon className="h-3.5 w-3.5" /> {t(tab.key)}
              </TabsTrigger>
            );
          })}
        </TabsList>
        {manageAccess.disabled && manageAccess.reason && (
          <div
            role="status"
            className="mt-4 rounded-md border border-bd-1 bg-bg-2 px-3 py-2 font-sans text-xs text-tx-2"
          >
            {manageAccess.reason}
          </div>
        )}
        <fieldset
          disabled={manageAccess.disabled}
          aria-disabled={manageAccess.disabled || undefined}
          className="contents"
        >
        <TabsContent value="profiles" className="mt-4">
          <ProfilesPanel
            profiles={profiles}
            onCreate={() =>
              setProfileEditor({ profile: 'new', section: 'profile' })
            }
            onEdit={(profile) =>
              setProfileEditor({ profile, section: 'profile' })
            }
          />
        </TabsContent>
        <TabsContent value="models" className="mt-4">
          <ModelsPanel providers={providers} />
        </TabsContent>
        <TabsContent value="prompts" className="mt-4">
          <PromptManagementPanel />
        </TabsContent>
        <TabsContent value="tools" className="mt-4">
          <ToolCapabilitiesPanel registry={tools} />
        </TabsContent>
        <TabsContent value="data" className="mt-4">
          <DataAccessPanel
            profile={activeProfile}
            onEdit={
              activeProfile
                ? () =>
                    setProfileEditor({
                      profile: activeProfile,
                      section: 'data',
                    })
                : undefined
            }
          />
        </TabsContent>
        <TabsContent value="network" className="mt-4">
          <NetworkPanel
            profile={activeProfile}
            registry={tools.data}
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
        <TabsContent value="approvals" className="mt-4">
          <ApprovalPolicyPanel
            profile={activeProfile}
            onEdit={
              activeProfile
                ? () =>
                    setProfileEditor({
                      profile: activeProfile,
                      section: 'approvals',
                    })
                : undefined
            }
          />
        </TabsContent>
        </fieldset>
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

function ProfilesPanel({
  profiles,
  onCreate,
  onEdit,
}: {
  profiles: UseQueryResult<intelligenceApi.AgentProfile[], Error>;
  onCreate: () => void;
  onEdit: (profile: intelligenceApi.AgentProfile) => void;
}) {
  const { t } = useTranslation('intelligence');
  if (profiles.isLoading) return <ProductState variant="loading" />;
  if (profiles.isError) return <ProductState variant="error" error={profiles.error} />;
  return (
    <SettingsSection
      title={t('settings.profiles.title')}
      description={t('settings.profiles.description')}
      action={<Button size="sm" onClick={onCreate}><Plus />{t('settings.profiles.create')}</Button>}
    >
      {profiles.data?.length ? (
        <div className="grid gap-3 xl:grid-cols-2">
          {profiles.data.map((profile) => (
            <article key={profile.id} className="rounded-lg border border-bd-0 bg-bg-2 p-4">
              <div className="flex items-start gap-3">
                <div className="grid h-9 w-9 shrink-0 place-items-center rounded-md border border-indigo/30 bg-indigo/10">
                  <Bot className="h-4 w-4 text-indigo" />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="flex items-start gap-2">
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <h3 className="font-strong text-tx-0">{profile.name}</h3>
                        {profile.is_default && <Badge variant="accent">{t('settings.profiles.default')}</Badge>}
                        <Badge variant="outline">{profile.enabled ? t('status.enabled') : t('status.disabled')}</Badge>
                      </div>
                      <p className="mt-1 text-xs leading-5 text-tx-3">{profile.description}</p>
                    </div>
                    <Button
                      variant="ghost"
                      size="icon"
                      aria-label={t('settings.profiles.edit')}
                      onClick={() => onEdit(profile)}
                    >
                      <Pencil />
                    </Button>
                  </div>
                </div>
              </div>
              <dl className="mt-4 grid grid-cols-3 gap-3 border-t border-bd-0 pt-3">
                <Stat label={t('settings.profiles.max_tools')} value={String(profile.max_tool_calls)} />
                <Stat label={t('settings.profiles.max_time')} value={`${Math.round(profile.max_investigation_secs / 60)}m`} />
                <Stat
                  label={t('settings.profiles.network')}
                  value={t(
                    profile.network_access === 'allowed'
                      ? 'settings.allowed'
                      : 'settings.blocked',
                  )}
                />
              </dl>
            </article>
          ))}
        </div>
      ) : (
        <ProductState
          variant="empty"
          compact
          title={t('settings.profiles.empty_title')}
          description={t('settings.profiles.empty_description')}
        />
      )}
    </SettingsSection>
  );
}

function ModelsPanel({
  providers,
}: {
  providers: UseQueryResult<providersApi.ModelProvider[], Error>;
}) {
  const { t } = useTranslation('intelligence');
  const [editor, setEditor] = React.useState<ProviderEditorTarget>(null);
  if (providers.isLoading) return <ProductState variant="loading" />;
  if (providers.isError) return <ProductState variant="error" error={providers.error} />;
  return (
    <SettingsSection
      title={t('settings.models.title')}
      description={t('settings.models.description')}
      action={<Button size="sm" onClick={() => setEditor('new')}><Plus />{t('settings.models.create')}</Button>}
    >
      {providers.data?.length ? (
        <div className="divide-y divide-bd-0 overflow-hidden rounded-lg border border-bd-0 bg-bg-2">
          {providers.data.map((provider) => (
            <div key={provider.id} className="flex flex-wrap items-center gap-4 px-4 py-3">
              <div className="grid h-8 w-8 place-items-center rounded-md border border-bd-0 bg-bg-1">
                <BrainCircuit className="h-4 w-4 text-tx-2" />
              </div>
              <div className="min-w-0 flex-1">
                <div className="font-strong text-tx-0">{provider.name}</div>
                <div className="mt-0.5 font-mono text-xs text-tx-3">{provider.default_model}</div>
                <div className="mt-0.5 max-w-xl truncate font-mono text-xs text-tx-3">
                  {provider.base_url ?? t('settings.models.default_endpoint')}
                </div>
              </div>
              <Badge variant="outline">{provider.provider}</Badge>
              <span className="inline-flex items-center gap-1.5 text-xs text-tx-3">
                <KeyRound className="h-3.5 w-3.5" />
                {provider.key_set ? t('settings.models.key_set') : t('settings.models.key_unset')}
              </span>
              <Badge variant="outline">{provider.enabled ? t('status.enabled') : t('status.disabled')}</Badge>
              <Button
                variant="ghost"
                size="icon"
                aria-label={t('settings.models.edit')}
                onClick={() => setEditor(provider)}
              >
                <Pencil />
              </Button>
            </div>
          ))}
        </div>
      ) : (
        <ProductState variant="empty" compact title={t('settings.models.empty_title')} />
      )}
      <ModelProviderEditorDrawer
        target={editor}
        onClose={() => setEditor(null)}
      />
    </SettingsSection>
  );
}

function DataAccessPanel({
  profile,
  onEdit,
}: {
  profile: intelligenceApi.AgentProfile | undefined;
  onEdit: (() => void) | undefined;
}) {
  const { t } = useTranslation('intelligence');
  return (
    <SettingsSection
      title={t('settings.data.title')}
      description={t('settings.data.description')}
      action={
        <EditSectionButton
          label={t('settings.data.configure')}
          onClick={onEdit}
        />
      }
    >
      <div className="grid gap-3 lg:grid-cols-2">
        <PolicyRow label={t('settings.data.environments')} value={scopeValue(profile?.data_scope.environments)} />
        <PolicyRow label={t('settings.data.services')} value={scopeValue(profile?.data_scope.services, t('settings.data.all_authorized'))} />
        <PolicyRow label={t('settings.data.streams')} value={scopeValue(profile?.data_scope.streams, t('settings.data.all_authorized'))} />
        <PolicyRow label={t('settings.data.cross_org')} value={t('settings.blocked')} secure />
      </div>
    </SettingsSection>
  );
}

function NetworkPanel({
  profile,
  registry,
  onEdit,
}: {
  profile: intelligenceApi.AgentProfile | undefined;
  registry: intelligenceApi.ToolRegistry | undefined;
  onEdit: (() => void) | undefined;
}) {
  const { t } = useTranslation('intelligence');
  const networkAllowed = profile?.network_access === 'allowed';
  const controls = [
    { key: 'settings.network.dynamic_http', blocked: registry?.dynamic_http !== true },
    { key: 'settings.network.shell', blocked: registry?.shell !== true },
    { key: 'settings.network.browser', blocked: registry?.browser !== true },
    { key: 'settings.network.open_mcp', blocked: registry?.open_mcp !== true },
  ];
  return (
    <SettingsSection
      title={t('settings.network.title')}
      description={t('settings.network.description')}
      action={
        <EditSectionButton
          label={t('settings.network.configure')}
          onClick={onEdit}
        />
      }
    >
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
              <Globe2 className="h-4 w-4 text-yellow-soft" />
            ) : (
              <GlobeLock className="h-4 w-4 text-green-soft" />
            )}
          </div>
          <div>
            <div className="font-strong text-tx-0">{t('settings.network.access')}</div>
            <div
              className={cn(
                'text-xs',
                networkAllowed ? 'text-yellow-soft' : 'text-green-soft',
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
      <div className="mt-4">
        <h3 className="text-sm font-strong text-tx-1">
          {t('settings.network.capabilities')}
        </h3>
        <p className="mt-1 text-xs leading-5 text-tx-3">
          {t('settings.network.capabilities_description')}
        </p>
      </div>
      <div className="mt-3 grid gap-2 lg:grid-cols-2">
        {controls.map((control) => (
          <div key={control.key} className="flex items-center gap-3 rounded-md border border-bd-0 bg-bg-2 px-3 py-2.5">
            {control.blocked ? <Check className="h-4 w-4 text-green-soft" /> : <X className="h-4 w-4 text-red-soft" />}
            <span className="text-sm text-tx-1">{t(control.key)}</span>
            <Badge variant="outline" className="ml-auto">{control.blocked ? t('settings.blocked') : t('settings.allowed')}</Badge>
          </div>
        ))}
      </div>
    </SettingsSection>
  );
}

function ApprovalPolicyPanel({
  profile,
  onEdit,
}: {
  profile: intelligenceApi.AgentProfile | undefined;
  onEdit: (() => void) | undefined;
}) {
  const { t } = useTranslation('intelligence');
  const rows = [
    ['l0', 'settings.approval_policy.automatic'],
    ['l1', 'settings.approval_policy.configurable'],
    ['l2', 'settings.approval_policy.single'],
    ['l3', 'settings.approval_policy.two_person'],
  ] as const;
  return (
    <SettingsSection
      title={t('settings.approval_policy.title')}
      description={t('settings.approval_policy.description')}
      action={
        <EditSectionButton
          label={t('settings.approval_policy.configure')}
          onClick={onEdit}
        />
      }
    >
      <div className="divide-y divide-bd-0 overflow-hidden rounded-lg border border-bd-0 bg-bg-2">
        {rows.map(([risk, fallback]) => {
          const policy = String(profile?.risk_policy[risk] ?? (
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
    </SettingsSection>
  );
}

function EditSectionButton({
  label,
  onClick,
}: {
  label: string;
  onClick: (() => void) | undefined;
}) {
  if (!onClick) return null;
  return (
    <Button size="sm" variant="outline" onClick={onClick}>
      <Pencil /> {label}
    </Button>
  );
}

function SettingsSection({
  title,
  description,
  action,
  children,
}: {
  title: string;
  description: string;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-lg border border-bd-0 bg-bg-1">
      <div className="flex flex-wrap items-start gap-4 border-b border-bd-0 px-4 py-3">
        <div className="min-w-0 flex-1">
          <h2 className="font-strong text-tx-0">{title}</h2>
          <p className="mt-1 text-xs leading-5 text-tx-3">{description}</p>
        </div>
        {action}
      </div>
      <div className="p-4">{children}</div>
    </section>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-xs text-tx-3">{label}</dt>
      <dd className="mt-0.5 font-mono text-xs font-strong text-tx-1">{value}</dd>
    </div>
  );
}

function PolicyRow({ label, value, secure = false }: { label: string; value: string; secure?: boolean }) {
  return (
    <div className="flex items-center gap-3 rounded-md border border-bd-0 bg-bg-2 p-3">
      <SlidersHorizontal className={cn('h-4 w-4 text-tx-3', secure && 'text-green-soft')} />
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
