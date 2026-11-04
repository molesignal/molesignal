import { useQueryClient } from '@tanstack/react-query';
import { Pause, Play, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, TableBulkBar, useBulkRun } from '@/admin';
import * as alertsApi from '@/api/alerts';
import { ChromeButton } from '@/shell/chrome';
import type { AlertRule } from '@/types/alerting';

import type { DisplayRule } from '../alertRuleModel';

/** The rule as it is saved, switched on or off. A rule has no call of its own for that. */
function switched(rule: AlertRule, enabled: boolean): alertsApi.AlertRuleInput {
  return { ...rule, enabled } as alertsApi.AlertRuleInput;
}

/** What can be done to the rules ticked in the list: switch them on or off, delete them. */
export function RuleBulkBar({ rules, onClear }: { rules: DisplayRule[]; onClear: () => void }) {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const [confirmingDelete, setConfirmingDelete] = React.useState(false);

  const run = useBulkRun({
    rows: rules,
    onAllDone: onClear,
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['alerts', 'rules'] }),
  });

  return (
    <>
      <TableBulkBar count={rules.length} onClear={onClear} data-testid="rule-bulk">
        <ChromeButton
          size="sm"
          disabled={run.isPending}
          onClick={() => run.mutate((rule) => alertsApi.update(rule.id, switched(rule.raw, true)))}
        >
          <Play className="h-3 w-3" />
          {tc('actions.enable')}
        </ChromeButton>
        <ChromeButton
          size="sm"
          disabled={run.isPending}
          onClick={() => run.mutate((rule) => alertsApi.update(rule.id, switched(rule.raw, false)))}
        >
          <Pause className="h-3 w-3" />
          {tc('actions.disable')}
        </ChromeButton>
        <ChromeButton size="sm" disabled={run.isPending} onClick={() => setConfirmingDelete(true)}>
          <Trash2 className="h-3 w-3" />
          {tc('actions.delete')}
        </ChromeButton>
      </TableBulkBar>
      <ConfirmDialog
        open={confirmingDelete}
        onOpenChange={setConfirmingDelete}
        destructive
        title={t('center.rules.bulk_delete_title', { count: rules.length })}
        description={t('center.rules.bulk_delete_description')}
        confirmLabel={tc('actions.delete')}
        busy={run.isPending}
        onConfirm={() =>
          run.mutate((rule) => alertsApi.remove(rule.id), { onSettled: () => setConfirmingDelete(false) })
        }
      />
    </>
  );
}
