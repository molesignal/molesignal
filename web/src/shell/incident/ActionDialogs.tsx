import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as escalationsApi from '@/api/escalations';
import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { uiLabelClass } from '@/shell/chrome';
import {
  FormChecklist,
  FormField,
  FormInput,
  FormRadio,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import type { BulkResult, EscalationPolicy, Incident, Reassignment } from '@/types/alerting';

import { refreshIncidents } from './refresh';
import { useTeams } from './Routing';

/** At most this many people are named in one go, as the backend allows. */
const MAX_PEOPLE = 10;

/** The longest snooze the backend takes: 7 days, in minutes. */
const MAX_SNOOZE_MINUTES = 7 * 24 * 60;

/** A titled group of controls that, unlike `FormField`, is not one `<label>`: a label around a list
 * of boxes would send every click to its first input. */
function Group({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div role="group" aria-label={label} className="flex min-w-0 flex-col gap-1.5">
      <span className={uiLabelClass}>{label}</span>
      {children}
    </div>
  );
}

interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/** Tells how a bulk request went: all done, or how many failed and why the first did. */
function reportBulk(t: ReturnType<typeof useTranslation<'alerts'>>['t'], results: BulkResult[]) {
  const failed = results.filter((result) => result.error);

  if (failed.length === 0) {
    toast.success(t('incident_actions.toasts.bulk_done', { count: results.length }));
  } else {
    toast.error(
      t('incident_actions.toasts.bulk_failed', {
        count: failed.length,
        total: results.length,
        reason: failed[0]?.error ?? '',
      }),
    );
  }
}

/** People of the organization to tick, narrowed by a search box. */
function PeoplePicker({ selected, onChange }: { selected: string[]; onChange: (next: string[]) => void }) {
  const { t } = useTranslation('alerts');
  const { users } = useUsers();
  const [query, setQuery] = React.useState('');
  const needle = query.trim().toLowerCase();

  const shown = users.filter(
    (user) =>
      selected.includes(user.id) ||
      !needle ||
      user.name.toLowerCase().includes(needle) ||
      user.email.toLowerCase().includes(needle),
  );

  return (
    <div className="flex flex-col gap-2">
      <FormInput
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        placeholder={t('incident_actions.people_search')}
        aria-label={t('incident_actions.people_search')}
      />
      <div className="max-h-56 overflow-y-auto">
        <FormChecklist
          options={shown.map((user) => ({ value: user.id, label: user.name, hint: user.email }))}
          selected={selected}
          onChange={(next) => onChange(next.slice(0, MAX_PEOPLE))}
        />
      </div>
    </div>
  );
}

/**
 * Reassigns incidents to people, a team or an escalation policy, or adds people as responders. Several
 * incidents go through the bulk endpoint; an optional note goes on a single incident's timeline.
 */
export function AssignDialog({
  incidentIds,
  mode,
  open,
  onOpenChange,
}: DialogProps & { incidentIds: string[]; mode: 'reassign' | 'responders' }) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const teams = useTeams();
  const policies = useQuery({
    queryKey: ['escalation-policies'],
    queryFn: () => escalationsApi.list(),
    enabled: open && mode === 'reassign',
  });
  const [kind, setKind] = React.useState<Reassignment['kind']>('users');
  const [people, setPeople] = React.useState<string[]>([]);
  const [teamId, setTeamId] = React.useState('');
  const [policyId, setPolicyId] = React.useState('');
  const [note, setNote] = React.useState('');

  React.useEffect(() => {
    if (!open) return;
    setKind('users');
    setPeople([]);
    setTeamId('');
    setPolicyId('');
    setNote('');
  }, [open]);

  const target = (): Reassignment | null => {
    if (kind === 'users') return people.length ? { kind, userIds: people } : null;
    if (kind === 'team') return teamId ? { kind, teamId: teamId } : null;
    return policyId ? { kind, escalationPolicyId: policyId } : null;
  };

  const save = useMutation({
    mutationFn: async () => {
      const to = target();
      if (!to) return;

      if (incidentIds.length > 1) {
        reportBulk(t, await incidentsApi.bulk(incidentIds, { action: 'reassign', to }));
        return;
      }

      const [id] = incidentIds;
      if (!id) return;

      if (mode === 'responders' && to.kind === 'users') await incidentsApi.addResponders(id, to.userIds);
      else await incidentsApi.reassign(id, to);

      if (note.trim()) await incidentsApi.addNote(id, note.trim());

      toast.success(
        mode === 'responders'
          ? t('incident_actions.toasts.responders_added')
          : t('incident_actions.toasts.reassigned'),
      );
    },
    onSuccess: async () => {
      await refreshIncidents(queryClient, incidentIds.length === 1 ? incidentIds[0] : undefined);
      onOpenChange(false);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const responders = mode === 'responders';

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {responders ? t('incident_actions.add_responders_title') : t('incident_actions.reassign_title')}
          </DialogTitle>
          <DialogDescription>
            {responders
              ? t('incident_actions.add_responders_description')
              : t('incident_actions.reassign_description', { count: incidentIds.length })}
          </DialogDescription>
        </DialogHeader>

        <div className="grid min-w-0 gap-4 py-1">
          {!responders && (
            <FormField label={t('incident_actions.reassign_to')}>
              <FormSelect
                value={kind}
                onChange={(next) => setKind(next as Reassignment['kind'])}
                ariaLabel={t('incident_actions.reassign_to')}
                options={[
                  { value: 'users', label: t('incident_actions.kinds.users') },
                  { value: 'team', label: t('incident_actions.kinds.team') },
                  { value: 'policy', label: t('incident_actions.kinds.policy') },
                ]}
              />
            </FormField>
          )}
          {(responders || kind === 'users') && (
            <Group label={t('incident_actions.people', { max: MAX_PEOPLE })}>
              <PeoplePicker selected={people} onChange={setPeople} />
            </Group>
          )}
          {!responders && kind === 'team' && (
            <FormField label={t('incident_actions.kinds.team')} hint={t('incident_actions.team_hint')}>
              <FormSelect
                value={teamId}
                onChange={setTeamId}
                ariaLabel={t('incident_actions.kinds.team')}
                placeholder={t('incident_actions.choose')}
                options={(teams.data ?? []).map((team) => ({ value: team.id, label: team.name }))}
              />
            </FormField>
          )}
          {!responders && kind === 'policy' && (
            <FormField label={t('incident_actions.kinds.policy')} hint={t('incident_actions.policy_hint')}>
              <FormSelect
                value={policyId}
                onChange={setPolicyId}
                ariaLabel={t('incident_actions.kinds.policy')}
                placeholder={t('incident_actions.choose')}
                options={(policies.data ?? []).map((policy) => ({ value: policy.id, label: policy.name }))}
              />
            </FormField>
          )}
          {incidentIds.length === 1 && (
            <FormField label={t('incident_actions.note_optional')}>
              <FormTextarea
                rows={2}
                value={note}
                onChange={(event) => setNote(event.target.value)}
                placeholder={t('incident_actions.note_placeholder')}
              />
            </FormField>
          )}
        </div>

        <DialogFooter>
          <ChromeButton onClick={() => onOpenChange(false)}>{t('incident_actions.cancel')}</ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={!target() || save.isPending}
            onClick={() => save.mutate()}
            data-testid="incident-assign-submit"
          >
            {responders ? t('incident_actions.add_responders') : t('incident_actions.reassign')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** The policy an incident escalates through: its own, or the one named `default` for the alias. */
function policyOf(policies: EscalationPolicy[], incident: Incident): EscalationPolicy | undefined {
  return (
    policies.find((policy) => policy.id === incident.escalationPolicyId) ??
    (incident.escalationPolicyId === 'default'
      ? policies.find((policy) => policy.name.trim().toLowerCase() === 'default')
      : undefined)
  );
}

const SEVERITY_RANK = { info: 0, warning: 1, error: 2, critical: 3 } as const;

/** Escalates now to a chosen step of the incident's policy: the next one unless another is picked. */
export function EscalateDialog({ incident, open, onOpenChange }: DialogProps & { incident: Incident }) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const { nameOf } = useUsers();
  const teams = useTeams();
  const policies = useQuery({
    queryKey: ['escalation-policies'],
    queryFn: () => escalationsApi.list(),
    enabled: open,
  });
  // Looked up while the dialog is open: a closed one renders with the page.
  const policy = open ? policyOf(policies.data ?? [], incident) : undefined;
  const [step, setStep] = React.useState('');

  const applies = (index: number) => {
    const min = policy?.steps[index]?.minSeverity;
    return !min || SEVERITY_RANK[incident.severity] >= SEVERITY_RANK[min];
  };

  React.useEffect(() => {
    if (!open || !policy) return;
    const next = policy.steps.findIndex((_, index) => index > incident.currentStep && applies(index));
    setStep(next >= 0 ? String(next) : '');
    // Picks the default once the policy has loaded for this opening.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, policy?.id]);

  const teamName = (id: string) => teams.data?.find((team) => team.id === id)?.name ?? id;

  const escalate = useMutation({
    mutationFn: () => incidentsApi.escalate(incident.id, Number(step)),
    onSuccess: async () => {
      toast.success(t('incident_actions.toasts.escalated'));
      await refreshIncidents(queryClient, incident.id);
      onOpenChange(false);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('incident_actions.escalate_title')}</DialogTitle>
          <DialogDescription>{t('incident_actions.escalate_description')}</DialogDescription>
        </DialogHeader>

        <div className="grid min-w-0 gap-4 py-1">
          {!policy ? (
            <p className="font-sans text-sm text-tx-2">
              {policies.isLoading ? t('incident_actions.loading') : t('incident_actions.no_policy')}
            </p>
          ) : (
            <FormRadio
              value={step}
              onChange={setStep}
              options={policy.steps
                .map((item, index) => ({ item, index }))
                .filter(({ index }) => applies(index))
                .map(({ item, index }) => ({
                  value: String(index),
                  label: t('incident_actions.step_n', { n: index + 1 }) +
                    (index === incident.currentStep ? ` · ${t('incident_actions.current_step')}` : ''),
                  hint: item.targets
                    .map((target) =>
                      target.kind === 'user'
                        ? nameOf(target.userId)
                        : target.kind === 'team'
                          ? teamName(target.teamId)
                          : t('incident_actions.schedule_target'),
                    )
                    .join(t('routing.list_separator')),
                }))}
            />
          )}
        </div>

        <DialogFooter>
          <ChromeButton onClick={() => onOpenChange(false)}>{t('incident_actions.cancel')}</ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={!policy || step === '' || escalate.isPending}
            onClick={() => escalate.mutate()}
            data-testid="incident-escalate-submit"
          >
            {t('incident_actions.escalate_submit')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Snoozes incidents for a length not on the menu, in minutes or hours, at most 7 days. */
export function SnoozeDialog({ incidentIds, open, onOpenChange }: DialogProps & { incidentIds: string[] }) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const [amount, setAmount] = React.useState('2');
  const [unit, setUnit] = React.useState<'minutes' | 'hours'>('hours');

  React.useEffect(() => {
    if (!open) return;
    setAmount('2');
    setUnit('hours');
  }, [open]);

  const minutes = Math.floor(Number(amount)) * (unit === 'hours' ? 60 : 1);
  const valid = Number.isFinite(minutes) && minutes >= 1 && minutes <= MAX_SNOOZE_MINUTES;

  const snooze = useMutation({
    mutationFn: async () => {
      const secs = minutes * 60;

      if (incidentIds.length > 1) {
        reportBulk(t, await incidentsApi.bulk(incidentIds, { action: 'snooze', durationSecs: secs }));
        return;
      }

      const [id] = incidentIds;
      if (id) await incidentsApi.snooze(id, secs);
      toast.success(t('incident_actions.toasts.snoozed'));
    },
    onSuccess: async () => {
      await refreshIncidents(queryClient, incidentIds.length === 1 ? incidentIds[0] : undefined);
      onOpenChange(false);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[min(380px,calc(100vw-24px))]">
        <DialogHeader>
          <DialogTitle>{t('incident_actions.snooze_title')}</DialogTitle>
          <DialogDescription>{t('incident_actions.snooze_description')}</DialogDescription>
        </DialogHeader>

        <div className="flex items-end gap-2 py-1">
          <FormField label={t('incident_actions.snooze_for')} className="flex-1">
            <FormInput
              type="number"
              min={1}
              value={amount}
              onChange={(event) => setAmount(event.target.value)}
              aria-label={t('incident_actions.snooze_for')}
            />
          </FormField>
          <FormSelect
            value={unit}
            onChange={(next) => setUnit(next as 'minutes' | 'hours')}
            ariaLabel={t('incident_actions.snooze_unit')}
            options={[
              { value: 'minutes', label: t('incident_actions.units.minutes') },
              { value: 'hours', label: t('incident_actions.units.hours') },
            ]}
          />
        </div>

        <DialogFooter>
          <ChromeButton onClick={() => onOpenChange(false)}>{t('incident_actions.cancel')}</ChromeButton>
          <ChromeButton variant="primary" disabled={!valid || snooze.isPending} onClick={() => snooze.mutate()}>
            {t('incident_actions.snooze')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/**
 * Merges incidents: other active incidents into this one, or, from a selection in the list, all but
 * the one picked into it.
 */
export function MergeDialog({
  incident,
  candidates,
  open,
  onOpenChange,
}: DialogProps & { incident?: Incident; candidates?: Incident[] }) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const active = useQuery({
    queryKey: ['alerts', 'incidents', 'active'],
    queryFn: () => incidentsApi.list({ scope: 'active' }),
    enabled: open && !candidates,
  });
  const [sources, setSources] = React.useState<string[]>([]);
  const [targetId, setTargetId] = React.useState('');

  React.useEffect(() => {
    if (!open) return;
    setSources([]);
    setTargetId(candidates?.[0]?.id ?? '');
  }, [open, candidates]);

  const others = open ? (active.data ?? []).filter((item) => item.id !== incident?.id && !item.mergedInto) : [];
  const into = incident?.id ?? targetId;
  const merged = candidates ? candidates.map((item) => item.id).filter((id) => id !== targetId) : sources;

  const merge = useMutation({
    mutationFn: () => incidentsApi.merge(into, merged),
    onSuccess: async () => {
      toast.success(t('incident_actions.toasts.merged', { count: merged.length }));
      await refreshIncidents(queryClient, into);
      onOpenChange(false);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('incident_actions.merge_title')}</DialogTitle>
          <DialogDescription>{t('incident_actions.merge_description')}</DialogDescription>
        </DialogHeader>

        <div className="grid min-w-0 gap-4 py-1">
          {candidates ? (
            <Group label={t('incident_actions.merge_target')}>
              <FormRadio
                value={targetId}
                onChange={setTargetId}
                options={candidates.map((item) => ({ value: item.id, label: item.summary }))}
              />
            </Group>
          ) : others.length === 0 ? (
            <p className="font-sans text-sm text-tx-2">
              {active.isLoading ? t('incident_actions.loading') : t('incident_actions.merge_nothing')}
            </p>
          ) : (
            <Group label={t('incident_actions.merge_sources')}>
              <div className="max-h-64 overflow-y-auto">
                <FormChecklist
                  options={others.map((item) => ({
                    value: item.id,
                    label: item.summary,
                    hint: t(`history.severity.${item.severity}`),
                  }))}
                  selected={sources}
                  onChange={setSources}
                />
              </div>
            </Group>
          )}
        </div>

        <DialogFooter>
          <ChromeButton onClick={() => onOpenChange(false)}>{t('incident_actions.cancel')}</ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={!into || merged.length === 0 || merge.isPending}
            onClick={() => merge.mutate()}
            data-testid="incident-merge-submit"
          >
            {t('incident_actions.merge_submit', { count: merged.length })}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
