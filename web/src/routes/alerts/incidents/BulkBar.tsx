import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Check, ChevronDown, CircleCheck, Combine, Users } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { TableBulkBar } from '@/admin';
import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { AssignDialog, MergeDialog, SnoozeDialog } from '@/shell/incident/ActionDialogs';
import { SNOOZE_PRESETS } from '@/shell/incident/Actions';
import { prioritiesOf, priorityLabel, urgenciesOf, urgencyLabel, useImportance } from '@/shell/incident/Importance';
import { refreshIncidents } from '@/shell/incident/refresh';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import type { BulkAction, Incident, Severity } from '@/types/alerting';

const SEVERITIES: Severity[] = ['critical', 'error', 'warning', 'info'];

/**
 * What can be done to the incidents ticked in the list at once: acknowledge, resolve, snooze,
 * reassign, change the severity, priority or urgency, or merge them into one of them.
 */
export function IncidentBulkBar({ incidents, onClear }: { incidents: Incident[]; onClear: () => void }) {
  const { t } = useTranslation('alerts');
  const levels = useImportance();
  const priorities = prioritiesOf(levels);
  const urgencies = urgenciesOf(levels);
  const queryClient = useQueryClient();
  const [dialog, setDialog] = React.useState<'reassign' | 'snooze' | 'merge' | null>(null);
  const ids = incidents.map((incident) => incident.id);

  const run = useMutation({
    mutationFn: (action: BulkAction) => incidentsApi.bulk(ids, action),
    onSuccess: async (results) => {
      const failed = results.filter((result) => result.error);

      if (failed.length === 0) {
        toast.success(t('incident_actions.toasts.bulk_done', { count: results.length }));
        onClear();
      } else {
        toast.error(
          t('incident_actions.toasts.bulk_failed', {
            count: failed.length,
            total: results.length,
            reason: failed[0]?.error ?? '',
          }),
        );
      }

      await refreshIncidents(queryClient);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <TableBulkBar count={incidents.length} onClear={onClear} label={t('incident_actions.bulk_label')}>
      <ChromeButton size="sm" disabled={run.isPending} onClick={() => run.mutate({ action: 'acknowledge' })}>
        <Check className="h-3 w-3" />
        {t('drawer.actions.ack')}
      </ChromeButton>
      <ChromeButton size="sm" disabled={run.isPending} onClick={() => run.mutate({ action: 'resolve' })}>
        <CircleCheck className="h-3 w-3" />
        {t('drawer.actions.resolve')}
      </ChromeButton>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <ChromeButton size="sm" disabled={run.isPending}>
            {t('incident_actions.snooze')}
            <ChevronDown className="h-3 w-3" />
          </ChromeButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-40">
          {SNOOZE_PRESETS.map((secs) => (
            <DropdownMenuItem key={secs} onSelect={() => run.mutate({ action: 'snooze', durationSecs: secs })}>
              {t('incident_actions.snooze_hours', { count: secs / 3600 })}
            </DropdownMenuItem>
          ))}
          <DropdownMenuItem onSelect={() => setDialog('snooze')}>{t('incident_actions.snooze_custom')}</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <ChromeButton size="sm" disabled={run.isPending} onClick={() => setDialog('reassign')}>
        <Users className="h-3 w-3" />
        {t('incident_actions.reassign')}
      </ChromeButton>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <ChromeButton size="sm" disabled={run.isPending} data-bulk-adjust>
            {t('incident_actions.adjust')}
            <ChevronDown className="h-3 w-3" />
          </ChromeButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-40">
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>{t('incident_actions.severity')}</DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-36">
              {SEVERITIES.map((severity) => (
                <DropdownMenuItem key={severity} onSelect={() => run.mutate({ action: 'set_severity', severity })}>
                  {t(`history.severity.${severity}`)}
                </DropdownMenuItem>
              ))}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>{t('incident_actions.priority')}</DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-36">
              {priorities.map((priority) => (
                <DropdownMenuItem key={priority} onSelect={() => run.mutate({ action: 'set_priority', priority })}>
                  {priorityLabel(levels, priority)}
                </DropdownMenuItem>
              ))}
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={() => run.mutate({ action: 'set_priority', priority: null })}>
                {t('importance.without_priority')}
              </DropdownMenuItem>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>{t('incident_actions.urgency')}</DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-36">
              {urgencies.map((urgency) => (
                <DropdownMenuItem key={urgency} onSelect={() => run.mutate({ action: 'set_urgency', urgency })}>
                  {urgencyLabel(levels, urgency, t)}
                </DropdownMenuItem>
              ))}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        </DropdownMenuContent>
      </DropdownMenu>
      <ChromeButton
        size="sm"
        disabled={run.isPending || incidents.length < 2}
        onClick={() => setDialog('merge')}
      >
        <Combine className="h-3 w-3" />
        {t('incident_actions.merge')}
      </ChromeButton>

      <AssignDialog
        incidentIds={ids}
        mode="reassign"
        open={dialog === 'reassign'}
        onOpenChange={(open) => {
          if (!open) setDialog(null);
        }}
      />
      <SnoozeDialog
        incidentIds={ids}
        open={dialog === 'snooze'}
        onOpenChange={(open) => {
          if (!open) setDialog(null);
        }}
      />
      <MergeDialog
        candidates={incidents}
        open={dialog === 'merge'}
        onOpenChange={(open) => {
          if (!open) {
            setDialog(null);
            onClear();
          }
        }}
      />
    </TableBulkBar>
  );
}
