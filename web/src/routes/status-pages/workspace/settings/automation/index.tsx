import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Archive, ChevronDown, ChevronUp, Eye, Pause, Play, Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import * as statusPagesApi from '@/api/statusPages';
import type { ActiveAutomationRule, AutomationRuleInput } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import { useStatusPageWorkspace } from '../../Layout';
import { SettingsCard } from '../SettingsSection';
import { CandidateReviewDrawer } from './CandidateReviewDrawer';
import { RuleEditor } from './RuleEditor';
import { AutomationSimulator } from './Simulator';

export function StatusPageAutomationSettings() {
  const { t } = useTranslation('status-pages');
  const { pageId, snapshot, manageAccess } = useStatusPageWorkspace();
  const publishAccess = useActionAccess({ permission: 'status_pages.publish' });
  const queryClient = useQueryClient();
  const [searchParams, setSearchParams] = useSearchParams();
  const candidateId = searchParams.get('candidate');
  const [editorOpen, setEditorOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<ActiveAutomationRule | null>(null);
  const rules = useQuery({
    queryKey: ['status-pages', pageId, 'automation', 'rules'],
    queryFn: () => statusPagesApi.listAutomationRules(pageId),
  });
  const candidates = useQuery({
    queryKey: ['status-pages', pageId, 'automation', 'candidates'],
    queryFn: () => statusPagesApi.listAutomationCandidates(pageId),
    refetchInterval: 30_000,
  });
  const settings = useQuery({
    queryKey: ['status-pages', pageId, 'automation', 'settings'],
    queryFn: () => statusPagesApi.getAutomationSettings(pageId),
  });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['status-pages', pageId, 'automation'] });
  const save = useMutation({
    mutationFn: async (input: AutomationRuleInput) => {
      if (editing) {
        const revision = await statusPagesApi.createAutomationRevision(pageId, editing.rule.id, input);
        return statusPagesApi.activateAutomationRevision(pageId, editing.rule.id, revision.id);
      }
      const created = await statusPagesApi.createAutomationRule(pageId, input);
      return statusPagesApi.activateAutomationRevision(pageId, created.rule.id, created.revision.id);
    },
    onSuccess: async () => {
      setEditorOpen(false);
      setEditing(null);
      toast.success(t('automation.toast.saved'));
      await refresh();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const lifecycle = useMutation({
    mutationFn: ({ rule, action }: { rule: ActiveAutomationRule; action: 'pause' | 'resume' | 'archive' }) =>
      statusPagesApi.setAutomationRuleLifecycle(
        pageId,
        rule.rule.id,
        action === 'pause' ? 'paused' : action === 'resume' ? 'active' : 'archived',
      ),
    onSuccess: refresh,
    onError: (error) => toast.error(toApiError(error).message),
  });
  const reorder = useMutation({
    mutationFn: ({ from, to }: { from: number; to: number }) => {
      const ids = (rules.data ?? []).map((rule) => rule.rule.id);
      const [moved] = ids.splice(from, 1);
      if (moved) ids.splice(to, 0, moved);
      return statusPagesApi.reorderAutomationRules(pageId, ids);
    },
    onSuccess: refresh,
    onError: (error) => toast.error(toApiError(error).message),
  });
  const pauseAll = useMutation({
    mutationFn: (paused: boolean) => statusPagesApi.setAutomationSettings(pageId, paused),
    onSuccess: refresh,
    onError: (error) => toast.error(toApiError(error).message),
  });

  if (rules.isLoading || candidates.isLoading || settings.isLoading) return <ProductState variant="loading" />;
  if (rules.isError || candidates.isError || settings.isError) {
    return <ProductState variant="error" error={rules.error ?? candidates.error ?? settings.error} />;
  }
  const activeRules = rules.data ?? [];
  const pending = (candidates.data ?? []).filter((candidate) =>
    ['delayed', 'pending_approval', 'approved', 'failed'].includes(candidate.state));

  const openCandidate = (id: string | null) => {
    const next = new URLSearchParams(searchParams);
    if (id) next.set('candidate', id);
    else next.delete('candidate');
    setSearchParams(next, { replace: true });
  };

  return (
    <div className="space-y-5">
      <SettingsCard title={t('automation.title')} description={t('automation.description')}>
        <div className="flex flex-col gap-3 rounded-md border border-bd-0 bg-bg-2 px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <div className="text-sm font-strong text-tx-0">{t('automation.pause.title')}</div>
            <div className="mt-1 text-xs text-tx-3">{t('automation.pause.description')}</div>
          </div>
          <Switch
            checked={settings.data?.paused ?? false}
            disabled={!manageAccess.allowed || pauseAll.isPending}
            onCheckedChange={(checked) => pauseAll.mutate(checked)}
            aria-label={t('automation.pause.title')}
          />
        </div>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="text-xs text-tx-3">{t('automation.rules.count', { count: activeRules.length })}</div>
          <ChromeButton
            variant="primary"
            disabled={!manageAccess.allowed}
            onClick={() => {
              setEditing(null);
              setEditorOpen(true);
            }}
          >
            <Plus className="h-3.5 w-3.5" />
            {t('automation.actions.add_rule')}
          </ChromeButton>
        </div>
        <div className="overflow-hidden rounded-md border border-bd-0">
          {activeRules.length === 0 ? (
            <div className="px-4 py-10 text-center text-xs text-tx-3">{t('automation.rules.empty')}</div>
          ) : activeRules.map((rule, index) => (
            <div key={rule.rule.id} className="flex flex-wrap items-center gap-3 border-b border-bd-0 px-3 py-3 last:border-b-0 sm:px-4">
              <div className="flex shrink-0 items-center gap-0.5">
                <IconButton
                  aria-label={t('actions.move_up')}
                  title={t('actions.move_up')}
                  disabled={!manageAccess.allowed || index === 0 || reorder.isPending}
                  onClick={() => reorder.mutate({ from: index, to: index - 1 })}
                >
                  <ChevronUp className="h-4 w-4" />
                </IconButton>
                <IconButton
                  aria-label={t('actions.move_down')}
                  title={t('actions.move_down')}
                  disabled={!manageAccess.allowed || index === activeRules.length - 1 || reorder.isPending}
                  onClick={() => reorder.mutate({ from: index, to: index + 1 })}
                >
                  <ChevronDown className="h-4 w-4" />
                </IconButton>
              </div>
              <div className="min-w-[180px] flex-1">
                <button
                  type="button"
                  className="truncate text-left text-sm font-strong text-tx-0 hover:text-indigo-soft focus-visible:text-indigo-soft"
                  onClick={() => {
                    setEditing(rule);
                    setEditorOpen(true);
                  }}
                >
                  {rule.rule.name}
                </button>
                <div className="mt-1 text-xs text-tx-3">
                  {t(`automation.source.${rule.revision.matchers.source_kind}`)} · {t(`automation.mode.${rule.revision.action.publication_mode}`)} · {t('automation.rules.delay', { count: Math.round(rule.revision.action.sustained_delay_seconds / 60) })}
                </div>
              </div>
              <Pill tone={rule.rule.lifecycle === 'active' ? 'green' : rule.rule.lifecycle === 'paused' ? 'yellow' : 'dim'}>
                {t(`automation.lifecycle.${rule.rule.lifecycle}`)}
              </Pill>
              <div className="flex items-center gap-1">
                {rule.rule.lifecycle === 'active' ? (
                  <ChromeButton size="sm" disabled={!manageAccess.allowed} onClick={() => lifecycle.mutate({ rule, action: 'pause' })}>
                    <Pause className="h-3.5 w-3.5" />{t('automation.actions.pause')}
                  </ChromeButton>
                ) : (
                  <ChromeButton size="sm" disabled={!manageAccess.allowed} onClick={() => lifecycle.mutate({ rule, action: 'resume' })}>
                    <Play className="h-3.5 w-3.5" />{t('automation.actions.resume')}
                  </ChromeButton>
                )}
                <ChromeButton size="sm" variant="ghost" disabled={!manageAccess.allowed} onClick={() => lifecycle.mutate({ rule, action: 'archive' })}>
                  <Archive className="h-3.5 w-3.5" />{t('actions.archive')}
                </ChromeButton>
              </div>
            </div>
          ))}
        </div>
        <AutomationSimulator pageId={pageId} rules={activeRules} />
      </SettingsCard>

      <SettingsCard title={t('automation.approvals.title')} description={t('automation.approvals.description')}>
        {pending.length === 0 ? (
          <div className="py-6 text-center text-xs text-tx-3">{t('automation.approvals.empty')}</div>
        ) : pending.map((candidate) => (
          <button
            key={candidate.id}
            type="button"
            className="flex min-h-14 w-full flex-wrap items-center gap-3 rounded-md border border-bd-0 bg-bg-2 px-4 py-3 text-left hover:bg-bg-3 focus-visible:bg-bg-3"
            onClick={() => openCandidate(candidate.id)}
          >
            <div className="min-w-[180px] flex-1">
              <div className="truncate text-sm font-strong text-tx-0">{candidate.title}</div>
              <div className="mt-1 text-xs text-tx-3">{t(`automation.candidate.${candidate.state}`)} · {t(`impact.${candidate.impact}`)}</div>
            </div>
            <Pill tone={candidate.state === 'failed' ? 'red' : candidate.state === 'pending_approval' ? 'yellow' : 'dim'}>
              {t(`automation.candidate.${candidate.state}`)}
            </Pill>
            <Eye className="h-4 w-4 text-tx-3" />
          </button>
        ))}
      </SettingsCard>

      <RuleEditor
        open={editorOpen}
        rule={editing}
        rules={activeRules}
        components={snapshot.components}
        saving={save.isPending}
        onOpenChange={(open) => {
          setEditorOpen(open);
          if (!open) setEditing(null);
        }}
        onSubmit={(input) => save.mutate(input)}
      />
      <CandidateReviewDrawer
        pageId={pageId}
        candidateId={candidateId}
        components={snapshot.components}
        canPublish={publishAccess.allowed}
        publishDisabledReason={publishAccess.reason}
        onClose={() => openCandidate(null)}
      />
    </div>
  );
}
