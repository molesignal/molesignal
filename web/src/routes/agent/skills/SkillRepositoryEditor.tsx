import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as skillsApi from '@/api/agent/skills';
import { toApiError } from '@/lib/http';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
} from '@/shell/FormDrawer';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';

import { syncSummary } from './syncSummary';
import { ToggleField } from '../mcp/ToggleField';
import { scopeChoice, scopeTarget, useScopes } from '../scope/useScopes';

const HOSTS: Array<skillsApi.RepoHost | 'auto'> = [
  'auto',
  'github',
  'gitlab',
  'gitea',
  'bitbucket',
  'archive',
];

/**
 * Adds or changes a skill repository. Skills come from a branch or a tag of it,
 * downloaded as an archive: every folder holding `SKILL.md` (under the folder
 * given, or in all of it) is a skill. Saving synchronizes it once.
 */
export function SkillRepositoryEditor({
  target,
  onClose,
  onSaved,
}: {
  target: skillsApi.SkillRepository | 'new' | null;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation('agent');
  const scopes = useScopes();
  const existing = target && target !== 'new' ? target : null;
  const [scope, setScope] = React.useState('personal');
  const [url, setUrl] = React.useState('');
  const [host, setHost] = React.useState<skillsApi.RepoHost | 'auto'>('auto');
  const [archiveUrl, setArchiveUrl] = React.useState('');
  const [gitRef, setGitRef] = React.useState('');
  const [dir, setDir] = React.useState('');
  const [token, setToken] = React.useState('');
  const [clearToken, setClearToken] = React.useState(false);
  const [autoSync, setAutoSync] = React.useState(false);

  React.useEffect(() => {
    setScope(
      existing
        ? scopeChoice({
            scope: existing.scope.scope,
            team_id: existing.scope.scope === 'team' ? existing.scope.team_id : null,
          })
        : 'personal',
    );
    setUrl(existing?.url ?? '');
    setHost(existing?.host ?? 'auto');
    setArchiveUrl(existing?.archive_url ?? '');
    setGitRef(existing?.git_ref ?? '');
    setDir(existing?.dir ?? '');
    setToken('');
    setClearToken(false);
    setAutoSync(existing?.auto_sync ?? false);
  }, [existing, target]);

  const save = useMutation({
    mutationFn: () => {
      const input: skillsApi.RepositoryInput = {
        ...scopeTarget(scope),
        url,
        host,
        git_ref: gitRef,
        dir,
        auto_sync: autoSync,
        ...(host === 'archive' && archiveUrl ? { archive_url: archiveUrl } : {}),
        ...(token ? { token } : {}),
        ...(clearToken && !token ? { clear_token: true } : {}),
      };
      return existing
        ? skillsApi.updateSkillRepository(existing.id, input)
        : skillsApi.createSkillRepository(input);
    },
    onSuccess: async (result) => {
      const summary = syncSummary(t, result.sync);
      if (result.sync.status === 'failed') toast.error(summary);
      else toast.success(summary);
      await onSaved();
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  if (!target) return null;

  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={t(
        existing ? 'settings.skills.repository.edit_title' : 'settings.skills.repository.create_title',
      )}
      subtitle={t('settings.skills.repository.description')}
      width={620}
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button
            disabled={save.isPending || !url.trim() || (host === 'archive' && !archiveUrl.trim())}
            onClick={() => save.mutate()}
          >
            {save.isPending ? t('settings.skills.repository.syncing') : t('common.save')}
          </Button>
        </>
      }
    >
      <FormSection>
        <FormField label={t('scope.field')}>
          <FormSelect
            value={scope}
            onChange={setScope}
            options={scopes.options}
            ariaLabel={t('scope.field')}
            className="w-full"
          />
        </FormField>
        <FormField label={t('settings.skills.repository.url')} required>
          <FormInput
            type="url"
            value={url}
            onChange={(event) => setUrl(event.target.value)}
            placeholder="https://github.com/acme/agent-skills"
          />
        </FormField>
        <FormRow>
          <FormField label={t('settings.skills.repository.host')}>
            <FormSelect
              value={host}
              onChange={(value) => setHost(value as skillsApi.RepoHost | 'auto')}
              options={HOSTS.map((value) => ({
                value,
                label: t(`settings.skills.repository.hosts.${value}`),
              }))}
              className="w-full"
            />
          </FormField>
          <FormField
            label={t('settings.skills.repository.ref')}
            hint={t('settings.skills.repository.ref_hint')}
          >
            <FormInput value={gitRef} onChange={(event) => setGitRef(event.target.value)} />
          </FormField>
        </FormRow>
        {host === 'archive' && (
          <FormField
            label={t('settings.skills.repository.archive_url')}
            hint={t('settings.skills.repository.archive_url_hint')}
            required
          >
            <FormInput
              type="url"
              value={archiveUrl}
              onChange={(event) => setArchiveUrl(event.target.value)}
              placeholder="https://git.example.com/acme/skills/archive/{ref}.tar.gz"
            />
          </FormField>
        )}
        <FormField
          label={t('settings.skills.repository.dir')}
          hint={t('settings.skills.repository.dir_hint')}
        >
          <FormInput
            value={dir}
            onChange={(event) => setDir(event.target.value)}
            placeholder="skills"
            className="font-mono"
          />
        </FormField>
        <FormField
          label={t('settings.skills.repository.token')}
          hint={
            existing?.token_set
              ? t('settings.skills.repository.token_saved', { last4: existing.token_last4 ?? '' })
              : t('settings.skills.repository.token_hint')
          }
        >
          <FormInput
            type="password"
            autoComplete="new-password"
            value={token}
            onChange={(event) => setToken(event.target.value)}
          />
        </FormField>
        {existing?.token_set && (
          <ToggleField
            label={t('settings.skills.repository.clear_token')}
            checked={clearToken}
            onChange={setClearToken}
          />
        )}
        <ToggleField
          label={t('settings.skills.repository.auto_sync')}
          hint={t('settings.skills.repository.auto_sync_hint')}
          checked={autoSync}
          onChange={setAutoSync}
        />
        <p className="text-xs leading-5 text-tx-3">
          {t(
            scope === 'org'
              ? 'settings.skills.repository.network_org'
              : 'settings.skills.repository.network_member',
          )}
        </p>
      </FormSection>
    </FormDrawer>
  );
}
