import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { LogOut, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as statusPagesApi from '@/api/statusPages';
import type { StatusPageAccessRule, StatusPageAccessRuleKind } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { useStatusPageWorkspace } from '../Layout';
import { SettingsCard } from './SettingsSection';

export function PrivateAccessSettings() {
  const { t } = useTranslation('status-pages');
  const queryClient = useQueryClient();
  const { pageId, snapshot } = useStatusPageWorkspace();
  const editable = snapshot.page.lifecycle === 'active';
  const [kind, setKind] = React.useState<StatusPageAccessRuleKind>('email');
  const [value, setValue] = React.useState('');
  const rules = useQuery({
    queryKey: ['status-pages', pageId, 'access-rules'],
    queryFn: () => statusPagesApi.listAccessRules(pageId),
  });
  const sessions = useQuery({
    queryKey: ['status-pages', pageId, 'access-sessions'],
    queryFn: () => statusPagesApi.listAccessSessions(pageId),
  });
  const refresh = () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: ['status-pages', pageId, 'access-rules'] }),
      queryClient.invalidateQueries({ queryKey: ['status-pages', pageId, 'access-sessions'] }),
    ]);
  const mutation = useMutation({
    mutationFn: async (action: { type: 'add' } | { type: 'remove'; rule: StatusPageAccessRule } | { type: 'session'; id: string } | { type: 'all' }) => {
      if (action.type === 'add') return statusPagesApi.createAccessRule(pageId, kind, value.trim());
      if (action.type === 'remove') return statusPagesApi.removeAccessRule(pageId, action.rule.id);
      if (action.type === 'session') return statusPagesApi.revokeAccessSession(pageId, action.id);
      return statusPagesApi.revokeAllAccessSessions(pageId);
    },
    onSuccess: async (_, action) => {
      toast.success(t(action.type === 'add' ? 'toast.access_rule_added' : 'toast.access_updated'));
      if (action.type === 'add') setValue('');
      await refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  return (
    <SettingsCard title={t('settings.private_access.title')} description={t('settings.private_access.description', { count: snapshot.page.private_session_days })}>
      <div className="grid items-end gap-2 sm:grid-cols-[150px_minmax(0,1fr)_auto]">
        <FormField label={t('fields.rule_type')}>
          <FormSelect
            value={kind}
            disabled={!editable}
            onChange={(next) => setKind(next as StatusPageAccessRuleKind)}
            options={[
              { value: 'email', label: t('access_rule.email') },
              { value: 'domain', label: t('access_rule.domain') },
            ]}
          />
        </FormField>
        <FormField label={t('fields.allowed_value')}>
          <FormInput
            type={kind === 'email' ? 'email' : 'text'}
            value={value}
            disabled={!editable}
            placeholder={kind === 'email' ? 'operator@example.com' : 'example.com'}
            onChange={(event) => setValue(event.currentTarget.value)}
          />
        </FormField>
        <ChromeButton
          variant="primary"
          disabled={!editable || !value.trim() || mutation.isPending}
          onClick={() => mutation.mutate({ type: 'add' })}
        >
          <Plus className="h-3.5 w-3.5" />
          {t('actions.add')}
        </ChromeButton>
      </div>

      <div className="divide-y divide-bd-0 rounded-lg border border-bd-0">
        {(rules.data ?? []).map((rule) => (
          <div key={rule.id} className="flex min-h-11 items-center gap-3 px-3">
            <Pill tone="neutral">{t(`access_rule.${rule.kind}`)}</Pill>
            <span className="min-w-0 flex-1 truncate font-mono text-xs text-tx-1">{rule.masked_value}</span>
            <IconButton
              aria-label={t('actions.delete')}
              title={t('actions.delete')}
              disabled={!editable || mutation.isPending}
              onClick={() => mutation.mutate({ type: 'remove', rule })}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </IconButton>
          </div>
        ))}
        {rules.data?.length === 0 && (
          <div className="px-3 py-7 text-center text-xs text-tx-3">{t('states.no_access_rules')}</div>
        )}
      </div>

      <div className="flex items-center border-t border-bd-0 pt-5">
        <div>
          <h3 className="text-sm font-display-strong text-tx-0">{t('settings.private_access.sessions')}</h3>
          <p className="mt-1 text-xs text-tx-3">{t('settings.private_access.sessions_description')}</p>
        </div>
        <ChromeButton
          className="ml-auto"
          disabled={!editable || !sessions.data?.length || mutation.isPending}
          onClick={() => mutation.mutate({ type: 'all' })}
        >
          <LogOut className="h-3.5 w-3.5" />
          {t('actions.revoke_all')}
        </ChromeButton>
      </div>
      <div className="divide-y divide-bd-0 rounded-lg border border-bd-0">
        {(sessions.data ?? []).map((session) => (
          <div key={session.id} className="flex min-h-14 items-center gap-3 px-3">
            <div className="min-w-0 flex-1">
              <div className="truncate font-mono text-xs text-tx-1">{session.masked_email}</div>
              <div className="mt-1 truncate text-xs text-tx-3">
                {session.origin_host} · {t('settings.private_access.expires', { date: formatMicrosActive(session.expires_at) })}
              </div>
            </div>
            <IconButton
              aria-label={t('actions.revoke')}
              title={t('actions.revoke')}
              disabled={!editable || mutation.isPending}
              onClick={() => mutation.mutate({ type: 'session', id: session.id })}
            >
              <LogOut className="h-3.5 w-3.5" />
            </IconButton>
          </div>
        ))}
        {sessions.data?.length === 0 && (
          <div className="px-3 py-7 text-center text-xs text-tx-3">{t('states.no_access_sessions')}</div>
        )}
      </div>
    </SettingsCard>
  );
}
