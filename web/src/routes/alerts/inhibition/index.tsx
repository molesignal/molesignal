import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as inhibitionsApi from '@/api/inhibitions';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { MatcherChips } from '@/shell/MatcherChips';
import { StatusWord, type StatusTone } from '@/shell/StatusWord';
import { toast } from '@/shell/ui/sonner';
import type { InhibitRule } from '@/types/alerting';

import { AlertsPage } from '../Frame';
import { alertFlatTableClassName } from '../Surfaces';
import { INHIBIT_RULES_KEY, InhibitRuleDrawer } from './Drawer';
import { type RuleStanding, ruleStanding } from './model';

const STANDING_TONE: Record<RuleStanding, StatusTone> = { holding: 'indigo', on: 'neutral', off: 'dim' };

/** What is holding something back now comes first; what is switched off, last. */
const STANDING_ORDER: Record<RuleStanding, number> = { holding: 0, on: 1, off: 2 };

/**
 * The inhibit rules: while an incident of one side is active, the incidents of the other side notify
 * nobody. A row reads as the sentence it is, and says how many incidents it holds back right now;
 * those incidents say so too, in the list and on their own page.
 */
export function AlertsInhibitions() {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.inhibitions.manage' });
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<InhibitRule | null>(null);
  const [removing, setRemoving] = React.useState<InhibitRule | null>(null);

  const query = useQuery({ queryKey: INHIBIT_RULES_KEY, queryFn: () => inhibitionsApi.list() });

  const rows = React.useMemo(
    () =>
      [...(query.data ?? [])].sort(
        (left, right) =>
          STANDING_ORDER[ruleStanding(left)] - STANDING_ORDER[ruleStanding(right)] || left.name.localeCompare(right.name),
      ),
    [query.data],
  );

  const remove = useMutation({
    mutationFn: (id: string) => inhibitionsApi.remove(id),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void queryClient.invalidateQueries({ queryKey: INHIBIT_RULES_KEY });
      setRemoving(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const newRule = (
    <ChromeButton
      variant="primary"
      disabled={manageAccess.disabled}
      disabledReason={manageAccess.reason}
      onClick={() => setCreating(true)}
    >
      <Plus className="h-4 w-4" />
      {t('inhibitions.new_rule')}
    </ChromeButton>
  );

  const state: ProductStateProps | null = query.isLoading
    ? { variant: 'loading' }
    : query.isError
      ? {
          variant: 'error',
          error: query.error,
          action: <ChromeButton onClick={() => void query.refetch()}>{t('center.retry')}</ChromeButton>,
        }
      : rows.length === 0
        ? {
            variant: 'empty',
            title: t('inhibitions.empty_title'),
            description: t('inhibitions.empty_description'),
            action: newRule,
          }
        : null;

  return (
    <>
      {/* An empty list carries the action itself; the header does not offer it a second time. */}
      <AlertsPage action={state?.variant === 'empty' ? undefined : newRule} state={state}>
        <DataTable
          className={alertFlatTableClassName}
          rows={rows}
          rowKey={(rule) => rule.id}
          onRowClick={(rule) => setEditing(rule)}
          isRowClickDisabled={() => manageAccess.disabled}
          rowClickDisabledReason={() => manageAccess.reason}
          columns={[
            {
              key: 'name',
              header: t('inhibitions.columns.name'),
              width: 240,
              sortable: true,
              value: (rule) => rule.name,
              cell: (rule) => (
                <div className="min-w-0">
                  <div className="truncate font-strong text-tx-0">{rule.name}</div>
                  {rule.comment && <div className="mt-0.5 truncate text-xs text-tx-2">{rule.comment}</div>}
                </div>
              ),
            },
            {
              key: 'source',
              header: t('inhibitions.columns.source'),
              cell: (rule) => (
                <div className="flex min-w-0 flex-col gap-1">
                  {rule.source.minSeverity && (
                    <span className="text-xs text-tx-2">
                      {t('inhibitions.at_least', { severity: t(`severity.${rule.source.minSeverity}`) })}
                    </span>
                  )}
                  <MatcherChips matchers={rule.source.matchers} empty={t('inhibitions.any_incident')} />
                </div>
              ),
            },
            {
              key: 'target',
              header: t('inhibitions.columns.target'),
              cell: (rule) => (
                <div className="flex min-w-0 flex-col gap-1">
                  {rule.target.maxSeverity && (
                    <span className="text-xs text-tx-2">
                      {t('inhibitions.at_most', { severity: t(`severity.${rule.target.maxSeverity}`) })}
                    </span>
                  )}
                  <MatcherChips matchers={rule.target.matchers} empty={t('inhibitions.any_incident')} />
                </div>
              ),
            },
            {
              key: 'equal',
              header: t('inhibitions.columns.equal'),
              width: 170,
              cell: (rule) => (
                <span className="flex min-w-0 flex-col">
                  <span className="truncate font-mono text-xs text-tx-1">{rule.equal.join(', ') || '—'}</span>
                  {rule.moreSevereOnly && <span className="text-xs text-tx-3">{t('inhibitions.only_worse')}</span>}
                </span>
              ),
            },
            {
              key: 'state',
              header: t('inhibitions.columns.status'),
              width: 150,
              sortable: true,
              sortValue: (rule) => STANDING_ORDER[ruleStanding(rule)],
              cell: (rule) => {
                const standing = ruleStanding(rule);

                return (
                  <span data-inhibit-standing={standing}>
                    <StatusWord tone={STANDING_TONE[standing]} strong={standing === 'holding'}>
                      {t(`inhibitions.standing.${standing}`, { count: rule.inhibiting })}
                    </StatusWord>
                  </span>
                );
              },
            },
            {
              key: 'actions',
              header: t('inhibitions.columns.actions'),
              width: 84,
              className: 'overflow-visible text-right',
              headerClassName: 'text-right',
              cell: (rule) => (
                <IconButton
                  aria-label={t('inhibitions.delete_named', { name: rule.name })}
                  title={tc('actions.delete')}
                  disabled={manageAccess.disabled}
                  disabledReason={manageAccess.reason}
                  className="ml-auto enabled:hover:text-red-soft"
                  onClick={(event) => {
                    event.stopPropagation();
                    setRemoving(rule);
                  }}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </IconButton>
              ),
            },
          ]}
        />
      </AlertsPage>
      <InhibitRuleDrawer
        open={creating || editing !== null}
        editing={editing}
        onClose={() => {
          setCreating(false);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('inhibitions.delete_title')}
        description={removing?.name ?? ''}
        consequences={
          // What it holds back now is released the moment it goes.
          removing && removing.inhibiting > 0
            ? [
                {
                  label: t('inhibitions.delete_effect_label'),
                  value: t('inhibitions.delete_effect', { count: removing.inhibiting }),
                },
              ]
            : undefined
        }
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (removing && manageAccess.allowed) remove.mutate(removing.id);
        }}
      />
    </>
  );
}
