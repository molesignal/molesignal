import { useMutation, useQueryClient } from '@tanstack/react-query';
import {
  ArrowUpRight,
  BellOff,
  BellRing,
  ChevronDown,
  Clock,
  Combine,
  Flag,
  Gauge,
  UserPlus,
  Users,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import type { Incident, Priority, Severity, Urgency } from '@/types/alerting';

import { AssignDialog, EscalateDialog, MergeDialog, SnoozeDialog } from './ActionDialogs';
import {
  prioritiesOf,
  priorityLabel,
  raisesUrgency,
  urgenciesOf,
  urgencyHint,
  urgencyLabel,
  urgencyOf,
  useImportance,
} from './Importance';
import { refreshIncidents } from './refresh';

/** Snooze choices of the menu, in seconds; any other length goes through the custom dialog. */
export const SNOOZE_PRESETS = [3600, 4 * 3600, 8 * 3600, 24 * 3600] as const;

const SEVERITIES: Severity[] = ['critical', 'error', 'warning', 'info'];
/** The radio value that clears the priority. */
const NO_PRIORITY = 'none';

type DialogKind = 'reassign' | 'responders' | 'escalate' | 'snooze' | 'merge';

/**
 * The menu of what else can be done to an active incident besides acknowledging and resolving it:
 * reassign, add responders, escalate now, snooze, change the severity, priority or urgency, merge
 * others in, silence.
 */
export function IncidentActions({
  incident,
  size,
  onSilence,
}: {
  incident: Incident;
  size?: 'sm' | undefined;
  onSilence?: (() => void) | undefined;
}) {
  const { t } = useTranslation('alerts');
  const levels = useImportance();
  const priorities = prioritiesOf(levels);
  const urgencies = urgenciesOf(levels);
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'alerts.incidents.acknowledge' });
  const silenceAccess = useActionAccess({ permission: 'alerts.incidents.silence' });
  const [dialog, setDialog] = React.useState<DialogKind | null>(null);

  const done = async (message: string) => {
    toast.success(message);
    await refreshIncidents(queryClient, incident.id);
  };

  const snooze = useMutation({
    mutationFn: (secs: number) => incidentsApi.snooze(incident.id, secs),
    onSuccess: () => done(t('incident_actions.toasts.snoozed')),
    onError: (error) => toast.error(toApiError(error).message),
  });

  const cancelSnooze = useMutation({
    mutationFn: () => incidentsApi.cancelSnooze(incident.id),
    onSuccess: () => done(t('incident_actions.toasts.snooze_cancelled')),
    onError: (error) => toast.error(toApiError(error).message),
  });

  const severity = useMutation({
    mutationFn: (next: Severity) => incidentsApi.setSeverity(incident.id, next),
    onSuccess: () => done(t('incident_actions.toasts.severity_changed')),
    onError: (error) => toast.error(toApiError(error).message),
  });

  const priority = useMutation({
    mutationFn: (next: Priority | null) => incidentsApi.setPriority(incident.id, next),
    onSuccess: () => done(t('incident_actions.toasts.priority_changed')),
    onError: (error) => toast.error(toApiError(error).message),
  });

  const urgency = useMutation({
    mutationFn: (next: Urgency) => incidentsApi.setUrgency(incident.id, next),
    onSuccess: (_, next) =>
      done(
        raisesUrgency(levels, urgencyOf(incident), next) && incident.status === 'open'
          ? t('incident_actions.toasts.urgency_raised')
          : t('incident_actions.toasts.urgency_changed'),
      ),
    onError: (error) => toast.error(toApiError(error).message),
  });

  const iconSize = size === 'sm' ? 'h-3 w-3' : 'h-3.5 w-3.5';
  const busy =
    snooze.isPending || cancelSnooze.isPending || severity.isPending || priority.isPending || urgency.isPending;

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <ChromeButton
            size={size ?? 'md'}
            disabled={busy || access.disabled}
            disabledReason={access.reason}
            data-testid="incident-more-actions"
          >
            {t('incident_actions.more')}
            <ChevronDown className={iconSize} />
          </ChromeButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-52">
          <DropdownMenuItem onSelect={() => setDialog('reassign')}>
            <Users className="h-3.5 w-3.5" />
            {t('incident_actions.reassign')}
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => setDialog('responders')}>
            <UserPlus className="h-3.5 w-3.5" />
            {t('incident_actions.add_responders')}
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => setDialog('escalate')}>
            <ArrowUpRight className="h-3.5 w-3.5" />
            {t('incident_actions.escalate')}
          </DropdownMenuItem>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>
              <Clock className="h-3.5 w-3.5" />
              {t('incident_actions.snooze')}
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-40">
              {SNOOZE_PRESETS.map((secs) => (
                <DropdownMenuItem key={secs} onSelect={() => snooze.mutate(secs)}>
                  {t('incident_actions.snooze_hours', { count: secs / 3600 })}
                </DropdownMenuItem>
              ))}
              <DropdownMenuItem onSelect={() => setDialog('snooze')}>
                {t('incident_actions.snooze_custom')}
              </DropdownMenuItem>
              {incident.snoozedUntil && (
                <>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem onSelect={() => cancelSnooze.mutate()}>
                    {t('incident_actions.snooze_cancel')}
                  </DropdownMenuItem>
                </>
              )}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>
              <Gauge className="h-3.5 w-3.5" />
              {t('incident_actions.severity')}
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-40">
              <DropdownMenuRadioGroup
                value={incident.severity}
                onValueChange={(next) => severity.mutate(next as Severity)}
              >
                {SEVERITIES.map((value) => (
                  <DropdownMenuRadioItem key={value} value={value}>
                    {t(`history.severity.${value}`)}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger data-testid="incident-priority">
              <Flag className="h-3.5 w-3.5" />
              {t('incident_actions.priority')}
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-40">
              <DropdownMenuRadioGroup
                value={incident.priority ?? NO_PRIORITY}
                onValueChange={(next) => priority.mutate(next === NO_PRIORITY ? null : (next as Priority))}
              >
                {priorities.map((value) => (
                  <DropdownMenuRadioItem key={value} value={value}>
                    {priorityLabel(levels, value)}
                  </DropdownMenuRadioItem>
                ))}
                <DropdownMenuRadioItem value={NO_PRIORITY}>{t('importance.without_priority')}</DropdownMenuRadioItem>
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger data-testid="incident-urgency">
              <BellRing className="h-3.5 w-3.5" />
              {t('incident_actions.urgency')}
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-56">
              <DropdownMenuRadioGroup
                value={urgencyOf(incident)}
                onValueChange={(next) => urgency.mutate(next as Urgency)}
              >
                {urgencies.map((value) => (
                  <DropdownMenuRadioItem key={value} value={value}>
                    <span className="flex flex-col">
                      <span>{urgencyLabel(levels, value, t)}</span>
                      <span className="text-xs text-tx-2">{urgencyHint(levels, value, t)}</span>
                    </span>
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuItem onSelect={() => setDialog('merge')}>
            <Combine className="h-3.5 w-3.5" />
            {t('incident_actions.merge')}
          </DropdownMenuItem>
          {onSilence && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                disabled={silenceAccess.disabled}
                onSelect={() => onSilence()}
                data-testid="incident-silence"
              >
                <BellOff className="h-3.5 w-3.5" />
                {t('drawer.actions.silence')}
              </DropdownMenuItem>
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
      <AssignDialog
        incidentIds={[incident.id]}
        mode={dialog === 'responders' ? 'responders' : 'reassign'}
        open={dialog === 'reassign' || dialog === 'responders'}
        onOpenChange={(open) => !open && setDialog(null)}
      />
      <EscalateDialog
        incident={incident}
        open={dialog === 'escalate'}
        onOpenChange={(open) => !open && setDialog(null)}
      />
      <SnoozeDialog
        incidentIds={[incident.id]}
        open={dialog === 'snooze'}
        onOpenChange={(open) => !open && setDialog(null)}
      />
      <MergeDialog
        incident={incident}
        open={dialog === 'merge'}
        onOpenChange={(open) => !open && setDialog(null)}
      />
    </>
  );
}
