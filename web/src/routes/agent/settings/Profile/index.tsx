import type { UseQueryResult } from '@tanstack/react-query';
import { Bot, Pencil, Plus } from 'lucide-react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import type { AgentProfile } from '@/api/agent';
import type { ModelProvider } from '@/api/agent/modelProviders';
import { ProductState } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

import {
  AgentProfileList,
  AgentProfileRow,
  AgentSettingsSection,
} from '../CardlessSurface';
import { profileRiskPolicy, profileScopeValues } from './model';
import { ProfilePagination, useProfilePagination } from './Pagination';

export function ProfilesPanel({
  profiles,
  providers,
  onCreate,
  onEdit,
}: {
  profiles: UseQueryResult<AgentProfile[], Error>;
  providers: ModelProvider[];
  onCreate: () => void;
  onEdit: (profile: AgentProfile) => void;
}) {
  const { t } = useTranslation('agent');
  const profileItems = profiles.data ?? [];
  const pagination = useProfilePagination(profileItems);

  if (profiles.isLoading) return <ProductState variant="loading" />;
  if (profiles.isError) {
    return <ProductState variant="error" error={profiles.error} />;
  }

  return (
    <AgentSettingsSection
      title={t('settings.profiles.title')}
      description={t('settings.profiles.description')}
      headerDivider={false}
      action={
        <Button size="sm" onClick={onCreate}>
          <Plus />
          {t('settings.profiles.create')}
        </Button>
      }
      bodyClassName={profileItems.length ? 'pt-0' : undefined}
    >
      {profileItems.length ? (
        <>
          <p className="border-b border-bd-0 pb-3 text-xs leading-5 text-tx-2">
            {t('settings.profiles.usage_hint')}
          </p>
          <AgentProfileList>
            {pagination.pageItems.map((profile) => {
              const provider = providers.find(
                (candidate) => candidate.id === profile.model_provider_id,
              );
              const model = provider
                ? `${provider.name} · ${profile.model ?? provider.default_model}`
                : profile.model ?? t('settings.profiles.provider_auto');
              const environments = profileScopeValues(
                profile,
                'environments',
              );
              const services = profileScopeValues(profile, 'services');
              const streams = profileScopeValues(profile, 'streams');
              const dataScope = [
                ...environments.map(
                  (value) =>
                    `${t('settings.profiles.scope_prefix.environment')}: ${value}`,
                ),
                ...services.map(
                  (value) =>
                    `${t('settings.profiles.scope_prefix.service')}: ${value}`,
                ),
                ...streams.map(
                  (value) =>
                    `${t('settings.profiles.scope_prefix.stream')}: ${value}`,
                ),
              ];
              const approval = `L2 ${t(
                `settings.profiles.policies.${profileRiskPolicy(profile, 'l2')}`,
              )} · L3 ${t(
                `settings.profiles.policies.${profileRiskPolicy(profile, 'l3')}`,
              )}`;
              return (
                <AgentProfileRow key={profile.id}>
                  <div className="flex items-start gap-3">
                    <span className="grid h-9 w-9 shrink-0 place-items-center rounded-md bg-indigo-dim text-indigo">
                      <Bot className="h-4 w-4" />
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className="flex items-start gap-2">
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-2">
                            <h3 className="font-sans text-sm font-display-strong text-tx-0">
                              {profile.name}
                            </h3>
                            {profile.is_default && (
                              <Badge variant="accent">
                                {t('settings.profiles.default')}
                              </Badge>
                            )}
                            <Badge variant="outline">
                              {profile.enabled
                                ? t('status.enabled')
                                : t('status.disabled')}
                            </Badge>
                          </div>
                          {profile.description && (
                            <p className="mt-1 max-w-3xl text-xs leading-5 text-tx-2">
                              {profile.description}
                            </p>
                          )}
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
                  <dl className="mt-4 grid grid-cols-1 gap-x-6 gap-y-3 border-t border-bd-0 pt-3 sm:ml-12 sm:grid-cols-2 xl:grid-cols-3">
                    <ProfileStat
                      label={t('settings.profiles.summary.model')}
                      value={model}
                      mono
                    />
                    <ProfileStat
                      label={t('settings.profiles.summary.tools', {
                        count: profile.allowed_tools.length,
                      })}
                      value={
                        <ProfileValueTags
                          values={profile.allowed_tools}
                          emptyLabel={t('settings.profiles.no_tools')}
                          limit={4}
                          mono
                          overflowTitle={t(
                            'settings.profiles.remaining_tools',
                            {
                              count: Math.max(
                                0,
                                profile.allowed_tools.length - 4,
                              ),
                            },
                          )}
                        />
                      }
                    />
                    <ProfileStat
                      label={t('settings.profiles.summary.data')}
                      value={
                        <ProfileValueTags
                          values={dataScope}
                          emptyLabel={t('settings.data.all_authorized')}
                          limit={6}
                        />
                      }
                    />
                    <ProfileStat
                      label={t('settings.profiles.summary.approval')}
                      value={approval}
                    />
                    <ProfileStat
                      label={t('settings.profiles.summary.runtime')}
                      value={t('settings.profiles.runtime_summary', {
                        minutes: Math.round(
                          profile.max_investigation_secs / 60,
                        ),
                        calls: profile.max_tool_calls,
                      })}
                      mono
                    />
                    <ProfileStat
                      label={t('settings.profiles.summary.network')}
                      value={t(
                        profile.network_access === 'allowed'
                          ? 'settings.allowed'
                          : 'settings.blocked',
                      )}
                    />
                  </dl>
                </AgentProfileRow>
              );
            })}
          </AgentProfileList>
          <ProfilePagination {...pagination} />
        </>
      ) : (
        <ProductState
          variant="empty"
          compact
          title={t('settings.profiles.empty_title')}
          description={t('settings.profiles.empty_description')}
        />
      )}
    </AgentSettingsSection>
  );
}

function ProfileStat({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: ReactNode;
  mono?: boolean;
}) {
  const textValue = typeof value === 'string' ? value : undefined;
  return (
    <div className="min-w-0">
      <dt className="text-xs text-tx-3">{label}</dt>
      <dd
        title={textValue}
        className={cn(
          'mt-1 text-sm font-strong text-tx-1',
          textValue && 'truncate',
          mono && 'font-mono',
        )}
      >
        {value}
      </dd>
    </div>
  );
}

function ProfileValueTags({
  values,
  emptyLabel,
  limit,
  mono = false,
  overflowTitle,
}: {
  values: string[];
  emptyLabel: string;
  limit: number;
  mono?: boolean;
  overflowTitle?: string;
}) {
  const displayed = values.length ? values.slice(0, limit) : [emptyLabel];
  const hiddenCount = Math.max(0, values.length - displayed.length);
  const hiddenValues = values.slice(limit);
  return (
    <span className="flex flex-wrap gap-1.5">
      {displayed.map((value) => (
        <span
          key={value}
          title={value}
          className={cn(
            'inline-flex min-h-6 max-w-64 items-center truncate rounded-full bg-indigo-dim px-2.5 py-0.5 text-xs font-strong text-indigo',
            mono && 'font-mono',
          )}
        >
          {value}
        </span>
      ))}
      {hiddenCount > 0 && (
        overflowTitle ? (
          <TooltipProvider delayDuration={150}>
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  aria-label={overflowTitle}
                  className="inline-flex min-h-6 items-center rounded-full bg-bg-3 px-2.5 py-0.5 font-mono text-xs font-strong text-tx-2 hover:bg-bg-4 hover:text-tx-0 focus-visible:bg-bg-4 focus-visible:text-tx-0"
                >
                  +{hiddenCount}
                </button>
              </TooltipTrigger>
              <TooltipContent
                side="top"
                align="start"
                className="max-h-72 max-w-xl overflow-y-auto p-3"
              >
                <div className="mb-2 font-sans text-xs font-strong text-tx-1">
                  {overflowTitle}
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {hiddenValues.map((value) => (
                    <span
                      key={value}
                      className="rounded-full bg-indigo-dim px-2.5 py-0.5 font-mono text-xs text-indigo"
                    >
                      {value}
                    </span>
                  ))}
                </div>
              </TooltipContent>
            </Tooltip>
          </TooltipProvider>
        ) : (
          <span className="inline-flex min-h-6 items-center rounded-full bg-bg-3 px-2.5 py-0.5 font-mono text-xs font-strong text-tx-2">
            +{hiddenCount}
          </span>
        )
      )}
    </span>
  );
}
