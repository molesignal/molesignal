import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as notifyApi from '@/api/notify';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormInput } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import { StepList } from './StepList';
import { savedSteps, stepsOf, type Step } from './steps';

/**
 * How one kind of notification reaches the person: the methods in order and, for alerts, how long
 * each waits, in a list for high urgency and one for low; then the quiet hours. A step through an
 * IM app that can page harder says how.
 */
export function PreferenceSection({
  userId,
  category,
  endpoints,
  connectors,
  boosts,
  preference,
  onSaved,
}: {
  userId: string;
  category: notifyApi.NotifyCategory;
  endpoints: notifyApi.UserNotifyEndpoint[];
  connectors: notifyApi.NotifyConnector[];
  /** The boosts each connector type offers. */
  boosts: Readonly<Record<string, readonly string[]>>;
  preference: notifyApi.UserNotifyPreference | undefined;
  onSaved: () => void;
}) {
  const { t } = useTranslation('notify');
  const quiet = preference?.quietHours;
  // Only alerts wait for someone to acknowledge them, and only they come in two urgencies.
  const isAlert = category === 'alert';
  const [enabled, setEnabled] = React.useState(preference?.enabled ?? true);
  const [high, setHigh] = React.useState<Step[]>(() => stepsOf(preference, 'high'));
  const [low, setLow] = React.useState<Step[]>(() => stepsOf(preference, 'low'));
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
    mutationFn: () => {
      const highSaved = savedSteps(high);
      const lowSaved = savedSteps(low);

      return notifyApi.updatePreference(userId, category, {
        enabled,
        endpointIds: highSaved.endpointIds,
        stepBoosts: highSaved.boosts,
        ...(isAlert
          ? {
              stepDelaysSecs: highSaved.delaysSecs,
              lowEndpointIds: lowSaved.endpointIds,
              lowStepDelaysSecs: lowSaved.delaysSecs,
              lowStepBoosts: lowSaved.boosts,
            }
          : {}),
        quietHours: quietEnabled ? { enabled: true, timezone, start, end } : null,
        allowHighUrgencyBypass: highUrgencyBypass,
      });
    },
    onSuccess: () => {
      toast.success(t('common.saved'));
      onSaved();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  // Said once under the lists, when one of the methods can page harder: how, and what each of their
  // platforms asks for it (an edition, a paid quota).
  const boostTypes = [
    ...new Set(
      endpoints.flatMap((endpoint) => {
        const connectorType = connectors.find((connector) => connector.id === endpoint.connectorId)?.connectorType;
        return connectorType && (boosts[connectorType] ?? []).length > 0 ? [connectorType] : [];
      }),
    ),
  ];
  const boostNotes = boostTypes
    .map((connectorType) => t(`preferences.boost_notes.${connectorType}`, { defaultValue: '' }))
    .filter((note) => note !== '');

  return (
    <article className="py-5 [&+&]:border-t [&+&]:border-bd-0" data-preference={category}>
      <div className="flex items-center justify-between gap-4">
        <h4 className="font-sans text-md font-display-strong text-tx-0">{t(`preferences.${category}`)}</h4>
        <Switch checked={enabled} onCheckedChange={setEnabled} />
      </div>
      {/* Columns that may shrink: a long identity is cut short, not the page made wider. */}
      <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
        <div className="space-y-4">
          <StepList
            title={isAlert ? t('preferences.high_urgency_steps') : t('preferences.endpoint_order')}
            urgency="high"
            steps={high}
            onChange={setHigh}
            endpoints={endpoints}
            connectors={connectors}
            boosts={boosts}
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
              boosts={boosts}
              waits
            />
          )}
          {boostTypes.length > 0 && (
            <div data-boost-hint className="space-y-1 text-xs leading-relaxed text-tx-3">
              <p>{t(isAlert ? 'preferences.boost_wait_hint' : 'preferences.boost_hint')}</p>
              {boostNotes.map((note) => (
                <p key={note}>{note}</p>
              ))}
            </div>
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
