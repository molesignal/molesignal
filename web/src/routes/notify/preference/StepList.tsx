import type { TFunction } from 'i18next';
import { ArrowDown, ArrowUp, Plus, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type * as notifyApi from '@/api/notify';
import { IconButton } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';
import { FittedSelect } from '@/shell/ui/fitted-select';
import type { Urgency } from '@/types/alerting';

import { endpointLabel } from '../model';
import { addedStep, freeBoosts, movedStep, stepKey, type Step } from './steps';

/** The select value of a step that does not page harder. */
const NO_BOOST = 'none';

/** A boost as the platform names it: DingTalk sends a DING, Lark a buzz; the others page urgently. */
function boostLabel(t: TFunction<'notify'>, connectorType: string, boost: string | null): string {
  const value = boost ?? NO_BOOST;

  return t(`preferences.boosts.${connectorType}.${value}`, {
    defaultValue: t(`preferences.boosts.default.${value}`, { defaultValue: value }),
  });
}

/**
 * The steps of one list in order, each with how it pages harder where its IM app can, and below
 * them the methods that can still be added: a click adds one as the next step. Where steps wait, a
 * method that pages harder can come again with another boost.
 */
export function StepList({
  title,
  hint,
  urgency,
  steps,
  onChange,
  endpoints,
  connectors,
  boosts,
  waits,
}: {
  title: string;
  hint?: string;
  urgency: Urgency;
  steps: Step[];
  onChange: (steps: Step[]) => void;
  endpoints: notifyApi.UserNotifyEndpoint[];
  connectors: notifyApi.NotifyConnector[];
  /** The boosts each connector type offers. */
  boosts: Readonly<Record<string, readonly string[]>>;
  /** Steps after the first wait for an acknowledgement. */
  waits: boolean;
}) {
  const { t } = useTranslation('notify');
  const endpointOf = (id: string) => endpoints.find((endpoint) => endpoint.id === id);
  const typeOf = (id: string) =>
    connectors.find((connector) => connector.id === endpointOf(id)?.connectorId)?.connectorType ?? '';
  const offeredOf = (id: string) => boosts[typeOf(id)] ?? [];
  const labelOf = (id: string) => endpointLabel(endpointOf(id), connectors);
  const update = (index: number, step: Step) =>
    onChange(steps.map((current, at) => (at === index ? step : current)));
  const addable = endpoints.flatMap((endpoint) => {
    const step = addedStep(steps, endpoint.id, offeredOf(endpoint.id), waits);
    return step ? [{ endpoint, step }] : [];
  });

  return (
    <div data-step-list={urgency}>
      <div className="mb-2 text-xs font-semibold text-tx-2">{title}</div>
      {hint && <p className="-mt-1 mb-2 text-xs leading-relaxed text-tx-3">{hint}</p>}
      {endpoints.length === 0 ? (
        <div className="rounded-md bg-bg-2 px-3 py-4 text-xs text-tx-3">{t('preferences.no_endpoints')}</div>
      ) : (
        // From a tablet up the steps share columns, so their boosts, waits and arrows line up; on a
        // phone a row wraps its controls under the method instead.
        <div className="divide-y divide-bd-0 bg-bg-2 sm:grid sm:grid-cols-[minmax(0,1fr)_auto_auto_auto]">
          {steps.map((step, index) => {
            const offered = offeredOf(step.endpointId);
            const free = freeBoosts(steps, step.endpointId, offered, index);
            // A boost the type no longer offers stays as it was saved until it is changed.
            const choices = [null, ...offered, ...(step.boost !== null && !offered.includes(step.boost) ? [step.boost] : [])];

            return (
              <div
                key={stepKey(step)}
                data-step={index + 1}
                className="flex min-h-11 flex-wrap items-center gap-x-2 gap-y-1 px-3 py-1.5 sm:col-span-full sm:grid sm:grid-cols-subgrid"
              >
                <span className="min-w-32 flex-1 truncate text-sm text-tx-1 sm:min-w-0">
                  {`${index + 1}. ${labelOf(step.endpointId)}`}
                </span>
                {choices.length > 1 ? (
                  <FittedSelect
                    value={step.boost ?? NO_BOOST}
                    onValueChange={(value) => update(index, { ...step, boost: value === NO_BOOST ? null : value })}
                    maxWidth={176}
                    className="justify-self-end"
                    aria-label={t('preferences.boost_label', { step: index + 1 })}
                    options={choices.map((boost) => ({
                      value: boost ?? NO_BOOST,
                      label: boostLabel(t, typeOf(step.endpointId), boost),
                      // Another step of the same method has it.
                      disabled: !free.includes(boost),
                    }))}
                  />
                ) : (
                  <span aria-hidden="true" className="hidden sm:block" />
                )}
                {waits && index >= 1 ? (
                  <label className="flex shrink-0 items-center gap-1.5 justify-self-end text-xs text-tx-2">
                    <FormInput
                      className="h-8 w-14 text-center text-sm"
                      inputMode="numeric"
                      aria-label={t('preferences.wait_label', { step: index + 1 })}
                      value={step.wait}
                      onChange={(event) => update(index, { ...step, wait: event.target.value.replace(/\D/g, '') })}
                    />
                    {t('preferences.wait_unit')}
                  </label>
                ) : (
                  <span aria-hidden="true" className="hidden sm:block" />
                )}
                <div className="flex shrink-0 items-center">
                  <IconButton
                    aria-label={t('preferences.move_up')}
                    disabled={index === 0}
                    onClick={() => onChange(movedStep(steps, index, -1))}
                  >
                    <ArrowUp className="h-3 w-3" />
                  </IconButton>
                  <IconButton
                    aria-label={t('preferences.move_down')}
                    disabled={index === steps.length - 1}
                    onClick={() => onChange(movedStep(steps, index, 1))}
                  >
                    <ArrowDown className="h-3 w-3" />
                  </IconButton>
                  <IconButton
                    aria-label={t('preferences.remove_step', { step: index + 1 })}
                    onClick={() => onChange(steps.filter((_, at) => at !== index))}
                  >
                    <X className="h-3 w-3" />
                  </IconButton>
                </div>
              </div>
            );
          })}
          {addable.map(({ endpoint, step }) => (
            <button
              key={endpoint.id}
              type="button"
              data-add-step={endpoint.id}
              aria-label={t('preferences.add_step', { method: labelOf(endpoint.id) })}
              className="flex min-h-11 w-full min-w-0 items-center gap-2 px-3 text-left text-sm text-tx-2 hover:text-tx-0 sm:col-span-full"
              onClick={() => onChange([...steps, step])}
            >
              <Plus className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
              <span className="min-w-0 truncate">{labelOf(endpoint.id)}</span>
            </button>
          ))}
        </div>
      )}
      {waits && steps.length > 1 && (
        <p className="mt-2 text-xs leading-relaxed text-tx-3">{t('preferences.wait_hint')}</p>
      )}
    </div>
  );
}
