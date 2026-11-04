import { useMutation } from '@tanstack/react-query';
import { ArrowDown, ArrowUp } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as notifyApi from '@/api/notify';
import { toApiError } from '@/lib/http';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { FormField, FormInput } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import type { Urgency } from '@/types/alerting';

import { connectorName } from './model';

/** A step waits at most a day. */
const MAX_WAIT_MINUTES = 1440;

/** The seconds a step waits, from the minutes typed for it. */
function waitSecs(minutes: string | undefined): number {
  return Math.min(MAX_WAIT_MINUTES, Number.parseInt(minutes ?? '0', 10) || 0) * 60;
}

/** One list of steps: the methods in order, and the minutes each after the first waits. */
interface Steps {
  selected: string[];
  waits: Record<string, string>;
}

function stepsOf(preference: notifyApi.UserNotifyPreference | undefined, urgency: Urgency): Steps {
  const steps = (preference?.steps ?? [])
    .filter((step) => (step.urgency ?? 'high') === urgency)
    .sort((left, right) => left.stepOrder - right.stepOrder);

  return {
    selected: steps.map((step) => step.endpointId),
    waits: Object.fromEntries(steps.map((step) => [step.endpointId, String(Math.round(step.delaySecs / 60))])),
  };
}

function delaysOf(steps: Steps): number[] {
  return steps.selected.map((id, index) => (index === 0 ? 0 : waitSecs(steps.waits[id])));
}

/**
 * How one kind of notification reaches the person: the methods in order and, for alerts, how long
 * each waits, in a list for high urgency and one for low; then the quiet hours.
 */
export function PreferenceSection({
  userId,
  category,
  endpoints,
  connectors,
  preference,
  onSaved,
}: {
  userId: string;
  category: notifyApi.NotifyCategory;
  endpoints: notifyApi.UserNotifyEndpoint[];
  connectors: notifyApi.NotifyConnector[];
  preference: notifyApi.UserNotifyPreference | undefined;
  onSaved: () => void;
}) {
  const { t } = useTranslation('notify');
  const quiet = preference?.quietHours;
  // Only alerts wait for someone to acknowledge them, and only they come in two urgencies.
  const isAlert = category === 'alert';
  const [enabled, setEnabled] = React.useState(preference?.enabled ?? true);
  const [high, setHigh] = React.useState<Steps>(() => stepsOf(preference, 'high'));
  const [low, setLow] = React.useState<Steps>(() => stepsOf(preference, 'low'));
  const [quietEnabled, setQuietEnabled] = React.useState(Boolean(quiet?.enabled));
  const [timezone, setTimezone] = React.useState(String(quiet?.timezone ?? 'UTC'));
  const [start, setStart] = React.useState(String(quiet?.start ?? '22:00'));
  const [end, setEnd] = React.useState(String(quiet?.end ?? '08:00'));
  const [highUrgencyBypass, setHighUrgencyBypass] = React.useState(preference?.allowHighUrgencyBypass ?? true);

  React.useEffect(() => {
    const nextQuiet = preference?.quietHours;
    setEnabled(preference?.enabled ?? true);
    setHigh(stepsOf(preference, 'high'));
    setLow(stepsOf(preference, 'low'));
    setQuietEnabled(Boolean(nextQuiet?.enabled));
    setTimezone(String(nextQuiet?.timezone ?? 'UTC'));
    setStart(String(nextQuiet?.start ?? '22:00'));
    setEnd(String(nextQuiet?.end ?? '08:00'));
    setHighUrgencyBypass(preference?.allowHighUrgencyBypass ?? true);
  }, [preference]);

  const save = useMutation({
    mutationFn: () =>
      notifyApi.updatePreference(userId, category, {
        enabled,
        endpointIds: high.selected,
        ...(isAlert
          ? {
              stepDelaysSecs: delaysOf(high),
              lowEndpointIds: low.selected,
              lowStepDelaysSecs: delaysOf(low),
            }
          : {}),
        quietHours: quietEnabled ? { enabled: true, timezone, start, end } : null,
        allowHighUrgencyBypass: highUrgencyBypass,
      }),
    onSuccess: () => {
      toast.success(t('common.saved'));
      onSaved();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <article className="py-5 [&+&]:border-t [&+&]:border-bd-0" data-preference={category}>
      <div className="flex items-center justify-between gap-4">
        <h4 className="font-sans text-md font-display-strong text-tx-0">{t(`preferences.${category}`)}</h4>
        <Switch checked={enabled} onCheckedChange={setEnabled} />
      </div>
      <div className="mt-4 grid gap-4 xl:grid-cols-2">
        <div className="space-y-4">
          <StepList
            title={isAlert ? t('preferences.high_urgency_steps') : t('preferences.endpoint_order')}
            urgency="high"
            steps={high}
            onChange={setHigh}
            endpoints={endpoints}
            connectors={connectors}
            waits={isAlert}
          />
          {isAlert && (
            <StepList
              title={t('preferences.low_urgency_steps')}
              hint={t('preferences.low_urgency_hint')}
              urgency="low"
              steps={low}
              onChange={setLow}
              endpoints={endpoints}
              connectors={connectors}
              waits
            />
          )}
        </div>
        <div className="space-y-3">
          <label className="flex min-h-11 items-center justify-between gap-3 bg-bg-2 px-3 text-sm text-tx-1">
            <span>{t('preferences.quiet_enabled')}</span>
            <Switch checked={quietEnabled} onCheckedChange={setQuietEnabled} />
          </label>
          {quietEnabled && (
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
              <FormField label={t('preferences.timezone')}>
                <FormInput className="h-11 md:h-9" value={timezone} onChange={(event) => setTimezone(event.target.value)} />
              </FormField>
              <FormField label={t('preferences.start')}>
                <FormInput className="h-11 md:h-9" value={start} onChange={(event) => setStart(event.target.value)} />
              </FormField>
              <FormField label={t('preferences.end')}>
                <FormInput className="h-11 md:h-9" value={end} onChange={(event) => setEnd(event.target.value)} />
              </FormField>
            </div>
          )}
          {isAlert && quietEnabled && (
            <>
              <label className="flex min-h-11 items-center justify-between gap-3 bg-bg-2 px-3 text-sm text-tx-1">
                <span>{t('preferences.high_urgency_bypass')}</span>
                <Switch
                  checked={highUrgencyBypass}
                  onCheckedChange={setHighUrgencyBypass}
                  data-high-urgency-bypass
                />
              </label>
              <p className="text-xs leading-relaxed text-tx-3">{t('preferences.quiet_low_urgency')}</p>
            </>
          )}
        </div>
      </div>
      <div className="mt-4 flex justify-end">
        <ChromeButton variant="primary" className="h-11 md:h-9" disabled={save.isPending} onClick={() => save.mutate()}>
          {save.isPending ? t('common.saving') : t('preferences.save', { category: t(`preferences.${category}`) })}
        </ChromeButton>
      </div>
    </article>
  );
}

/** The methods a list uses, in order: a click picks or drops one, the arrows move it. */
function StepList({
  title,
  hint,
  urgency,
  steps,
  onChange,
  endpoints,
  connectors,
  waits,
}: {
  title: string;
  hint?: string;
  urgency: Urgency;
  steps: Steps;
  onChange: (steps: Steps) => void;
  endpoints: notifyApi.UserNotifyEndpoint[];
  connectors: notifyApi.NotifyConnector[];
  /** Steps after the first wait for an acknowledgement. */
  waits: boolean;
}) {
  const { t } = useTranslation('notify');
  const { selected } = steps;

  const toggle = (id: string) =>
    onChange({
      ...steps,
      selected: selected.includes(id) ? selected.filter((value) => value !== id) : [...selected, id],
    });
  const move = (index: number, direction: -1 | 1) => {
    const target = index + direction;
    if (target < 0 || target >= selected.length) return;
    const next = [...selected];
    [next[index], next[target]] = [next[target]!, next[index]!];
    onChange({ ...steps, selected: next });
  };

  return (
    <div data-step-list={urgency}>
      <div className="mb-2 text-xs font-semibold text-tx-2">{title}</div>
      {hint && <p className="-mt-1 mb-2 text-xs leading-relaxed text-tx-3">{hint}</p>}
      {endpoints.length === 0 ? (
        <div className="rounded-md bg-bg-2 px-3 py-4 text-xs text-tx-3">{t('preferences.no_endpoints')}</div>
      ) : (
        <div className="divide-y divide-bd-0 bg-bg-2">
          {endpoints.map((endpoint) => {
            const index = selected.indexOf(endpoint.id);
            return (
              <div key={endpoint.id} className="flex min-h-11 items-center gap-2 px-3">
                <button
                  type="button"
                  className="min-w-0 flex-1 truncate text-left text-sm text-tx-1 hover:text-tx-0"
                  onClick={() => toggle(endpoint.id)}
                >
                  {index >= 0 ? `${index + 1}. ` : ''}
                  {connectorName(connectors, endpoint.connectorId)} · {endpoint.displayName ?? endpoint.externalIdentity}
                </button>
                {waits && index >= 1 && (
                  <label className="flex shrink-0 items-center gap-1.5 text-xs text-tx-2">
                    <FormInput
                      className="h-8 w-14 text-center text-sm"
                      inputMode="numeric"
                      aria-label={t('preferences.wait_label', { step: index + 1 })}
                      value={steps.waits[endpoint.id] ?? '0'}
                      onChange={(event) =>
                        onChange({
                          ...steps,
                          waits: { ...steps.waits, [endpoint.id]: event.target.value.replace(/\D/g, '') },
                        })
                      }
                    />
                    {t('preferences.wait_unit')}
                  </label>
                )}
                {index >= 0 && (
                  <>
                    <IconButton aria-label={t('preferences.move_up')} disabled={index === 0} onClick={() => move(index, -1)}>
                      <ArrowUp className="h-3 w-3" />
                    </IconButton>
                    <IconButton
                      aria-label={t('preferences.move_down')}
                      disabled={index === selected.length - 1}
                      onClick={() => move(index, 1)}
                    >
                      <ArrowDown className="h-3 w-3" />
                    </IconButton>
                  </>
                )}
              </div>
            );
          })}
        </div>
      )}
      {waits && selected.length > 1 && <p className="mt-2 text-xs leading-relaxed text-tx-3">{t('preferences.wait_hint')}</p>}
    </div>
  );
}
