import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable, TableBulkBar, useBulkRun, useTableSelection } from '@/admin';
import * as mutesApi from '@/api/mutes';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { MatcherChips } from '@/shell/MatcherChips';
import { StatusWord, type StatusTone } from '@/shell/StatusWord';
import { toast } from '@/shell/ui/sonner';
import type { MuteRule } from '@/types/alerting';

import { AlertsPage } from '../Frame';
import { alertFlatTableClassName } from '../Surfaces';
import { SilenceDrawer } from './Drawer';
import { type SilenceStanding, silenceStanding, windowSummary } from './window';

const STANDING_TONE: Record<SilenceStanding, StatusTone> = {
  active: 'indigo',
  upcoming: 'blue',
  waiting: 'neutral',
  ended: 'dim',
  off: 'dim',
};

/** What is silencing now comes first; what is over or off, last. */
const STANDING_ORDER: Record<SilenceStanding, number> = { active: 0, upcoming: 1, waiting: 2, ended: 3, off: 4 };

/**
 * The silence policies: what each one catches, when it applies, and whether it
 * is holding notifications back at this moment.
 *
 * A row answers what a responder asks of a silence without opening it. "On"
 * says little about a policy whose window ended last month, so the state is
 * read from the window: silencing now, waiting for its hours, not begun, over,
 * or switched off.
 */
export function AlertsSilences() {
  const { t, i18n } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.mutes.manage' });
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<MuteRule | null>(null);
  const [removing, setRemoving] = React.useState<MuteRule | null>(null);

  const query = useQuery({ queryKey: ['alert-mutes'], queryFn: () => mutesApi.list() });
  const nowMicros = Date.now() * 1000;
  const rows = React.useMemo(
    () =>
      [...(query.data ?? [])].sort(
        (left, right) =>
          STANDING_ORDER[silenceStanding(left, nowMicros)] - STANDING_ORDER[silenceStanding(right, nowMicros)]
          || left.name.localeCompare(right.name),
      ),
    [nowMicros, query.data],
  );
  const { selection, selectedRows, clear: clearSelection } = useTableSelection(rows, (rule) => rule.id);
  const [removingSelected, setRemovingSelected] = React.useState(false);
  const removeSelected = useBulkRun({
    rows: selectedRows,
    onAllDone: clearSelection,
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['alert-mutes'] }),
  });
  const remove = useMutation({
    mutationFn: (id: string) => mutesApi.remove(id),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void queryClient.invalidateQueries({ queryKey: ['alert-mutes'] });
      setRemoving(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const newSilence = (
    <ChromeButton
      variant="primary"
      disabled={manageAccess.disabled}
      disabledReason={manageAccess.reason}
      onClick={() => setCreating(true)}
    >
      <Plus className="h-4 w-4" />
      {t('silences.new_silence')}
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
            title: t('silences.empty_title'),
            description: t('silences.empty_description'),
            action: newSilence,
          }
        : null;

  return (
    <>
      {/* An empty list carries the action itself; the header does not offer it a second time. */}
      <AlertsPage action={state?.variant === 'empty' ? undefined : newSilence} state={state}>
        {selectedRows.length > 0 && (
          <TableBulkBar count={selectedRows.length} onClear={clearSelection} data-testid="silence-bulk">
            <ChromeButton size="sm" disabled={removeSelected.isPending} onClick={() => setRemovingSelected(true)}>
              <Trash2 className="h-3 w-3" />
              {tc('actions.delete')}
            </ChromeButton>
          </TableBulkBar>
        )}
        <DataTable
          className={alertFlatTableClassName}
          rows={rows}
          rowKey={(rule) => rule.id}
          {...(manageAccess.allowed ? { selection } : {})}
          onRowClick={(rule) => setEditing(rule)}
          isRowClickDisabled={() => manageAccess.disabled}
          rowClickDisabledReason={() => manageAccess.reason}
          columns={[
            {
              key: 'name',
              header: t('silences.columns.name'),
              width: 300,
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
              key: 'matchers',
              header: t('silences.columns.matchers'),
              cell: (rule) => <MatcherChips matchers={rule.matchers} empty={t('silences.matches_everything')} />,
            },
            {
              key: 'window',
              header: t('silences.columns.window'),
              width: 300,
              cell: (rule) => (
                <span className="flex flex-col">
                  <span className="tabular-nums text-tx-1">{windowSummary(rule.window, t, i18n.language)}</span>
                  {rule.window.type !== 'fixed' && rule.enabled && rule.nextChangeAt ? (
                    <span data-silence-next className="text-xs tabular-nums text-tx-3">
                      {t(rule.active ? 'silences.next_end' : 'silences.next_start', {
                        time: formatMicrosActive(rule.nextChangeAt, false),
                      })}
                    </span>
                  ) : null}
                </span>
              ),
            },
            {
              key: 'state',
              header: t('silences.columns.status'),
              width: 150,
              sortable: true,
              sortValue: (rule) => STANDING_ORDER[silenceStanding(rule, nowMicros)],
              cell: (rule) => {
                const standing = silenceStanding(rule, nowMicros);

                return (
                  <StatusWord tone={STANDING_TONE[standing]} strong={standing === 'active'}>
                    {t(`silences.standing.${standing}`)}
                  </StatusWord>
                );
              },
            },
            {
              key: 'actions',
              header: t('silences.columns.actions'),
              width: 84,
              className: 'overflow-visible text-right',
              headerClassName: 'text-right',
              cell: (rule) => (
                <IconButton
                  aria-label={t('silences.delete_named', { name: rule.name })}
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
      <SilenceDrawer
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
        title={t('silences.delete_title')}
        description={removing?.name ?? ''}
        consequences={
          // A silence holding notifications back right now stops holding them the moment it goes.
          removing && silenceStanding(removing, nowMicros) === 'active'
            ? [{ label: t('silences.delete_effect_label'), value: t('silences.delete_active_effect') }]
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
      <ConfirmDialog
        open={removingSelected}
        onOpenChange={setRemovingSelected}
        destructive
        title={t('silences.bulk_delete_title', { count: selectedRows.length })}
        consequences={
          selectedRows.some((rule) => silenceStanding(rule, nowMicros) === 'active')
            ? [{ label: t('silences.delete_effect_label'), value: t('silences.delete_active_effect') }]
            : undefined
        }
        confirmLabel={tc('actions.delete')}
        busy={removeSelected.isPending}
        onConfirm={() =>
          removeSelected.mutate((rule) => mutesApi.remove(rule.id), {
            onSettled: () => setRemovingSelected(false),
          })
        }
      />
    </>
  );
}
