import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Ellipsis,
  FolderGit2,
  FolderOpen,
  Package,
  Plus,
  RefreshCw,
  Search,
  Sparkles,
  Upload,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as skillsApi from '@/api/agent/skills';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { Input } from '@/shell/ui/input';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import { SkillDetailDrawer } from './SkillDetailDrawer';
import { SkillRepositoryEditor } from './SkillRepositoryEditor';
import { SkillUploadDrawer } from './SkillUploadDrawer';
import { syncSummary } from './syncSummary';
import { scopeTarget, useScopes } from '../scope/useScopes';

type SkillFilter = 'all' | 'instance' | 'org' | 'team' | 'personal';

const FILTERS: readonly SkillFilter[] = ['all', 'instance', 'org', 'team', 'personal'];

function inFilter(skill: skillsApi.Skill, filter: SkillFilter): boolean {
  return filter === 'all' || skill.scope === filter;
}

/**
 * The skills of Mole Agent: those that ship with it and those in the
 * instance's skill folders (read-only, turned on or off for the
 * organization), and the uploaded and repository ones of the organization,
 * the teams and each person. A chat uses the skills of its scope by itself,
 * and each is also a `/` command.
 */
export function SkillsPanel() {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const scopes = useScopes();
  const [filter, setFilter] = React.useState<SkillFilter>('all');
  const [search, setSearch] = React.useState('');
  const [uploading, setUploading] = React.useState(false);
  const [repositoryEditor, setRepositoryEditor] = React.useState<
    skillsApi.SkillRepository | 'new' | null
  >(null);
  const [inspected, setInspected] = React.useState<skillsApi.Skill | null>(null);
  const skills = useQuery({
    queryKey: ['agent', 'skills'],
    queryFn: skillsApi.listSkills,
    retry: false,
  });
  const repositories = useQuery({
    queryKey: ['agent', 'skill-repositories'],
    queryFn: skillsApi.listSkillRepositories,
    retry: false,
  });

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ['agent', 'skills'] }),
      queryClient.invalidateQueries({ queryKey: ['agent', 'skill-repositories'] }),
      queryClient.invalidateQueries({ queryKey: ['agent', 'commands'] }),
    ]);
  };
  const fail = (error: unknown) => toast.error(toApiError(error).message);

  const update = useMutation({
    mutationFn: ({ id, change }: { id: string; change: Parameters<typeof skillsApi.updateSkill>[1] }) =>
      skillsApi.updateSkill(id, change),
    onSuccess: refresh,
    onError: fail,
  });
  const remove = useMutation({
    mutationFn: skillsApi.deleteSkill,
    onSuccess: async () => {
      toast.success(t('settings.skills.deleted'));
      await refresh();
    },
    onError: fail,
  });
  const sync = useMutation({
    mutationFn: skillsApi.syncSkillRepository,
    onSuccess: async (report) => {
      const summary = syncSummary(t, report);
      if (report.status === 'failed') toast.error(summary);
      else toast.success(summary);
      await refresh();
    },
    onError: fail,
  });
  const removeRepository = useMutation({
    mutationFn: skillsApi.deleteSkillRepository,
    onSuccess: async () => {
      toast.success(t('settings.skills.repository.deleted'));
      await refresh();
    },
    onError: fail,
  });
  const rescan = useMutation({
    mutationFn: skillsApi.rescanSkillPaths,
    onSuccess: refresh,
    onError: fail,
  });

  const rows = skills.data?.skills ?? [];
  const needle = search.trim().toLowerCase();
  const shown = rows.filter(
    (skill) =>
      inFilter(skill, filter) &&
      (!needle ||
        [skill.name, skill.title, skill.description, ...skill.tags]
          .join(' ')
          .toLowerCase()
          .includes(needle)),
  );
  const paths = skills.data?.paths;

  return (
    <section aria-labelledby="agent-skills-title" className="min-w-0" data-agent-skills>
      <div className="flex min-h-[32px] flex-wrap items-center gap-x-4 gap-y-2 pb-[10px]">
        <h2 id="agent-skills-title" className="sr-only">
          {t('settings.skills.title')}
        </h2>
        <p className="min-w-0 flex-1 font-sans text-xs leading-5 text-tx-2">
          {t('settings.skills.description')}
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" onClick={() => setUploading(true)}>
            <Upload className="h-3.5 w-3.5" />
            {t('settings.skills.upload.open')}
          </Button>
          <Button size="sm" variant="outline" onClick={() => setRepositoryEditor('new')}>
            <FolderGit2 className="h-3.5 w-3.5" />
            {t('settings.skills.repository.open')}
          </Button>
        </div>
      </div>

      <div className="flex flex-col gap-3 border-t border-bd-0 pt-[10px] md:flex-row md:items-center">
        <div className="relative min-w-0 flex-1 md:max-w-md">
          <Search
            aria-hidden
            className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-tx-3"
          />
          <Input
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t('settings.skills.search_placeholder')}
            aria-label={t('settings.skills.search_placeholder')}
            className="h-9 pl-9"
          />
        </div>
        <div className="flex min-w-0 flex-wrap items-center gap-1" role="group">
          {FILTERS.map((value) => (
            <Button
              key={value}
              size="sm"
              variant={filter === value ? 'default' : 'ghost'}
              className="h-8 gap-1.5 px-3 text-xs"
              aria-pressed={filter === value}
              data-agent-skill-filter={value}
              onClick={() => setFilter(value)}
            >
              {t(`settings.skills.filters.${value}`)}
              <span className={cn('tabular-nums', filter === value ? 'opacity-80' : 'text-tx-3')}>
                {rows.filter((skill) => inFilter(skill, value)).length}
              </span>
            </Button>
          ))}
        </div>
      </div>

      {skills.isLoading ? (
        <ProductState variant="loading" />
      ) : skills.isError ? (
        <ProductState variant="error" error={skills.error} />
      ) : shown.length === 0 ? (
        <ProductState
          variant="empty"
          compact
          title={t(rows.length === 0 ? 'settings.skills.empty_title' : 'settings.skills.empty_filter')}
          className="bg-transparent [box-shadow:none]"
        />
      ) : (
        <ul className="mt-3 divide-y divide-bd-0 overflow-hidden rounded-lg bg-bg-2">
          {shown.map((skill) => (
            <li
              key={skill.id}
              data-agent-skill={skill.name}
              className="flex flex-wrap items-center gap-3 px-4 py-3"
            >
              <span className="grid h-8 w-8 shrink-0 place-items-center rounded-md bg-bg-1">
                {skill.source === 'builtin' ? (
                  <Sparkles className="h-4 w-4 text-indigo" />
                ) : skill.source === 'repository' ? (
                  <FolderGit2 className="h-4 w-4 text-tx-2" />
                ) : skill.source === 'path' ? (
                  <FolderOpen className="h-4 w-4 text-tx-2" />
                ) : (
                  <Package className="h-4 w-4 text-tx-2" />
                )}
              </span>
              <button
                type="button"
                onClick={() => setInspected(skill)}
                className="min-w-0 flex-1 text-left"
              >
                <span className="flex flex-wrap items-center gap-2">
                  <span className="font-strong text-tx-0">{skill.title}</span>
                  <span className="font-mono text-xs text-tx-3">/{skill.name}</span>
                  <span className="rounded bg-bg-3 px-1.5 text-type-micro text-tx-2">
                    {scopes.label(skill)}
                  </span>
                  <span className="rounded bg-bg-3 px-1.5 text-type-micro text-tx-2">
                    {t(`settings.skills.source.${skill.source}`)}
                  </span>
                  {skill.version && (
                    <span className="text-type-micro text-tx-3">v{skill.version}</span>
                  )}
                  {skill.requires_runner && (
                    <span className="rounded bg-[var(--yellow-tint)] px-1.5 text-type-micro text-[var(--yellow-ink)]">
                      {t('settings.skills.needs_runner')}
                    </span>
                  )}
                </span>
                <span className="mt-0.5 block truncate text-xs text-tx-3">{skill.description}</span>
              </button>
              <Switch
                aria-label={t('settings.skills.switch', { name: skill.name })}
                checked={skill.enabled}
                disabled={!skill.moderatable || update.isPending}
                onCheckedChange={(enabled) => update.mutate({ id: skill.id, change: { enabled } })}
              />
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button variant="ghost" size="icon" aria-label={t('common.actions')}>
                    <Ellipsis className="h-4 w-4" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem onSelect={() => setInspected(skill)}>
                    {t('settings.skills.inspect')}
                  </DropdownMenuItem>
                  {skill.editable && skill.source === 'upload' && (
                    <DropdownMenuSub>
                      <DropdownMenuSubTrigger>{t('settings.skills.move')}</DropdownMenuSubTrigger>
                      <DropdownMenuSubContent>
                        {scopes.options.map((option) => (
                          <DropdownMenuItem
                            key={option.value}
                            onSelect={() =>
                              update.mutate({ id: skill.id, change: scopeTarget(option.value) })
                            }
                          >
                            {option.label}
                          </DropdownMenuItem>
                        ))}
                      </DropdownMenuSubContent>
                    </DropdownMenuSub>
                  )}
                  {skill.moderatable && skill.source === 'upload' && (
                    <DropdownMenuItem
                      className="text-[var(--red-ink)]"
                      onSelect={() => remove.mutate(skill.id)}
                    >
                      {t('settings.skills.delete')}
                    </DropdownMenuItem>
                  )}
                </DropdownMenuContent>
              </DropdownMenu>
            </li>
          ))}
        </ul>
      )}

      <section aria-labelledby="agent-skill-repositories" className="mt-6">
        <div className="flex items-center gap-2">
          <h3 id="agent-skill-repositories" className="text-sm font-strong text-tx-1">
            {t('settings.skills.repository.title')}
          </h3>
          <Button
            size="sm"
            variant="ghost"
            className="ml-auto"
            onClick={() => setRepositoryEditor('new')}
          >
            <Plus className="h-3.5 w-3.5" />
            {t('settings.skills.repository.open')}
          </Button>
        </div>
        {repositories.isError ? (
          <ProductState variant="error" error={repositories.error} compact />
        ) : (repositories.data ?? []).length === 0 ? (
          <p className="mt-1 text-xs text-tx-3">{t('settings.skills.repository.empty')}</p>
        ) : (
          <ul className="mt-2 divide-y divide-bd-0 overflow-hidden rounded-lg bg-bg-2">
            {(repositories.data ?? []).map((repository) => (
              <li
                key={repository.id}
                data-agent-skill-repository={repository.status}
                className="flex flex-wrap items-center gap-3 px-4 py-3"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="truncate font-mono text-xs font-strong text-tx-0">
                      {repository.url}
                    </span>
                    <span className="rounded bg-bg-3 px-1.5 text-type-micro text-tx-2">
                      {scopes.label({
                        scope: repository.scope.scope,
                        team_id: repository.scope.scope === 'team' ? repository.scope.team_id : null,
                        owner_id:
                          repository.scope.scope === 'personal' ? repository.scope.owner_id : null,
                      })}
                    </span>
                    <span
                      className={cn(
                        'rounded px-1.5 text-type-micro font-strong',
                        repository.status === 'ok' && 'bg-[var(--green-tint)] text-[var(--green-ink)]',
                        repository.status === 'partial' &&
                          'bg-[var(--yellow-tint)] text-[var(--yellow-ink)]',
                        repository.status === 'failed' && 'bg-red-dim text-[var(--red-ink)]',
                        repository.status === 'pending' && 'bg-bg-3 text-tx-2',
                      )}
                    >
                      {t(`settings.skills.repository.status.${repository.status}`)}
                    </span>
                  </div>
                  <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-xs text-tx-3">
                    <span>{repository.git_ref || t('settings.skills.repository.default_branch')}</span>
                    {repository.dir && <span className="font-mono">{repository.dir}</span>}
                    {repository.last_commit && (
                      <span className="font-mono">{repository.last_commit.slice(0, 10)}</span>
                    )}
                    {repository.last_synced_at && (
                      <span>
                        {t('settings.skills.repository.synced_at', {
                          time: formatMicrosActive(repository.last_synced_at),
                        })}
                      </span>
                    )}
                    {repository.auto_sync && <span>{t('settings.skills.repository.daily')}</span>}
                  </div>
                  {repository.last_error && (
                    <p className="mt-1 whitespace-pre-line text-xs text-[var(--yellow-ink)]">
                      {repository.last_error}
                    </p>
                  )}
                </div>
                {repository.editable && (
                  <>
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={sync.isPending}
                      onClick={() => sync.mutate(repository.id)}
                    >
                      <RefreshCw className="h-3.5 w-3.5" />
                      {t('settings.skills.repository.sync')}
                    </Button>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button variant="ghost" size="icon" aria-label={t('common.actions')}>
                          <Ellipsis className="h-4 w-4" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuItem onSelect={() => setRepositoryEditor(repository)}>
                          {t('common.edit')}
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          className="text-[var(--red-ink)]"
                          onSelect={() => removeRepository.mutate(repository.id)}
                        >
                          {t('settings.skills.repository.delete')}
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </>
                )}
              </li>
            ))}
          </ul>
        )}
      </section>

      {paths && paths.roots.length > 0 && (
        <section aria-labelledby="agent-skill-paths" className="mt-6" data-agent-skill-paths>
          <div className="flex items-center gap-2">
            <h3 id="agent-skill-paths" className="text-sm font-strong text-tx-1">
              {t('settings.skills.paths.title')}
            </h3>
            {scopes.manager && (
              <Button
                size="sm"
                variant="ghost"
                className="ml-auto"
                disabled={rescan.isPending}
                onClick={() => rescan.mutate()}
              >
                <RefreshCw className="h-3.5 w-3.5" />
                {t('settings.skills.paths.rescan')}
              </Button>
            )}
          </div>
          <p className="mt-1 text-xs leading-5 text-tx-3">{t('settings.skills.paths.description')}</p>
          <ul className="mt-2 space-y-1 text-xs">
            {paths.roots.map((root) => (
              <li key={root} className="font-mono text-tx-1">
                {root}
              </li>
            ))}
          </ul>
          {paths.scanned_at_micros && (
            <p className="mt-1 text-xs text-tx-3">
              {t('settings.skills.paths.scanned_at', {
                time: formatMicrosActive(paths.scanned_at_micros),
              })}
            </p>
          )}
          {paths.problems.length > 0 && (
            <ul className="mt-2 space-y-1">
              {paths.problems.map((problem) => (
                <li
                  key={`${problem.dir}:${problem.message}`}
                  className="rounded-md bg-[var(--yellow-tint)] px-3 py-1.5 text-xs text-[var(--yellow-ink)]"
                >
                  <span className="font-mono">{problem.dir}</span> — {problem.message}
                </li>
              ))}
            </ul>
          )}
        </section>
      )}

      <SkillUploadDrawer open={uploading} onClose={() => setUploading(false)} onUploaded={refresh} />
      <SkillRepositoryEditor
        target={repositoryEditor}
        onClose={() => setRepositoryEditor(null)}
        onSaved={refresh}
      />
      <SkillDetailDrawer skill={inspected} onClose={() => setInspected(null)} />
    </section>
  );
}
