import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as providersApi from '@/api/agent/modelProviders';
import * as inspectionsApi from '@/api/logInspections';
import type { InspectionLimits, LogInspection } from '@/api/logInspections';
import * as notifyApi from '@/api/notify';
import * as streamsApi from '@/api/streams';
import { toApiError } from '@/lib/http';
import { activeTimeZone } from '@/lib/time';
import { hasPermission, useProductAccess } from '@/product/access';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import {
  NAME_MAX,
  NOTIFY_WHEN,
  REQUIREMENTS_MAX,
  WINDOW_SECONDS,
  channelConnectors,
  draftFromInspection,
  draftProblems,
  durationLabel,
  inputFromDraft,
  newDraft,
  runAsName,
  windowFor,
  type InspectionDraft,
} from './model';
import { RoutesField } from './RoutesField';
import { ScheduleField } from './ScheduleField';

export const INSPECTIONS_KEY = ['log-inspections'] as const;

const FORM_ID = 'log-inspection-editor';

export interface InspectionSeed {
  stream?: string | undefined;
  statement?: string | undefined;
  requirements?: string | undefined;
  providerId?: string | undefined;
}

interface InspectionEditorProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The inspection being changed; null for a new one. */
  inspection: LogInspection | null;
  /** What a new inspection starts from: the query the reader is looking at. */
  seed?: InspectionSeed | undefined;
  limits: InspectionLimits;
  onSaved?: ((inspection: LogInspection) => void) | undefined;
}

/**
 * The form a scheduled inspection is saved with: what to inspect, how often
 * and with which model, and whom to tell.
 *
 * Saving makes the reader the one the inspection runs as, so the form says so
 * beside the button that does it, and says whose it is now when that is
 * somebody else.
 *
 * The form lives for one opening. It is made from what it was opened for and
 * goes with the drawer: nothing typed is carried into the next opening, and
 * nothing is written over it when the page behind reads the inspection again.
 */
export function InspectionEditor({ open, ...opening }: InspectionEditorProps) {
  return open ? <InspectionForm {...opening} /> : null;
}

function InspectionForm({
  onOpenChange,
  inspection,
  seed,
  limits,
  onSaved,
}: Omit<InspectionEditorProps, 'open'>) {
  const { t, i18n } = useTranslation('logs');
  const queryClient = useQueryClient();
  const access = useProductAccess();
  const mayListConnectors = hasPermission('alerts.read', access);
  // The draft is there before the fields are drawn. A select that is given its value after
  // it was drawn, in the same step as the one choice holding that value, reads it back as
  // nothing chosen: the stream of an inspection would be lost on opening it.
  const [draft, setDraft] = React.useState<InspectionDraft>(() =>
    inspection ? draftFromInspection(inspection) : newDraft(limits, seed),
  );
  // The window follows the schedule until the reader sets one of their own.
  const [windowChosen, setWindowChosen] = React.useState(inspection !== null);

  const streams = useQuery({
    queryKey: ['streams', 'logs-selector'],
    queryFn: () => streamsApi.list(500),
  });
  const providers = useQuery({
    queryKey: ['agent', 'model-providers'],
    queryFn: () => providersApi.list(),
    retry: false,
  });
  const connectors = useQuery({
    queryKey: ['notify', 'connectors'],
    queryFn: () => notifyApi.listConnectors(),
    enabled: mayListConnectors,
    retry: false,
  });

  const logStreams = React.useMemo(() => {
    const names = (streams.data ?? [])
      .filter((stream) => stream.type === 'logs' && streamsApi.isQueryable(stream))
      .map((stream) => stream.name);

    // The stream of the inspection stays offered while the list loads, or after it was removed.
    return draft.stream && !names.includes(draft.stream) ? [draft.stream, ...names] : names;
  }, [streams.data, draft.stream]);
  const models = React.useMemo(
    () => (providers.data ?? []).filter((provider) => provider.enabled || provider.id === draft.providerId),
    [providers.data, draft.providerId],
  );
  const channels = React.useMemo(
    () => channelConnectors(connectors.data ?? [], draft.routes),
    [connectors.data, draft.routes],
  );

  // A new inspection takes the first model there is, as the inspection of one query does.
  React.useEffect(() => {
    if (draft.providerId) return;

    const first = (providers.data ?? []).find((provider) => provider.enabled);

    if (first) setDraft((current) => (current.providerId ? current : { ...current, providerId: first.id }));
  }, [draft.providerId, providers.data]);

  const change = (patch: Partial<InspectionDraft>) =>
    setDraft((current) => {
      const next = { ...current, ...patch };
      const rescheduled = 'scheduleKind' in patch || 'everyMinutes' in patch;

      return rescheduled && !windowChosen
        ? { ...next, windowSecs: windowFor(next.scheduleKind, next.everyMinutes) }
        : next;
    });

  const timezone = inspection?.timezone ?? activeTimeZone();
  const problems = draftProblems(draft, limits);
  const save = useMutation({
    mutationFn: () => {
      const input = inputFromDraft(draft, {
        timezone,
        language: inspection?.language ?? i18n.resolvedLanguage ?? i18n.language,
        model: inspection?.provider_id === draft.providerId ? inspection.model : null,
      });

      return inspection ? inspectionsApi.update(inspection.id, input) : inspectionsApi.create(input);
    },
    onSuccess: (saved) => {
      toast.success(t(inspection ? 'inspections.toast.saved' : 'inspections.toast.created'));
      void queryClient.invalidateQueries({ queryKey: INSPECTIONS_KEY });
      onOpenChange(false);
      onSaved?.(saved);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const model = models.find((provider) => provider.id === draft.providerId);

  return (
    <FormDrawer
      open
      onOpenChange={onOpenChange}
      title={t(inspection ? 'inspections.editor.edit_title' : 'inspections.editor.new_title')}
      subtitle={t('inspections.editor.subtitle')}
      footer={
        <>
          <p data-inspection-run-as className="mr-auto min-w-0 max-w-[420px] font-sans text-xs leading-relaxed text-tx-3">
            {inspection && inspection.run_as_user
              ? t('inspections.editor.run_as_change', { name: runAsName(inspection) })
              : t('inspections.editor.run_as_you')}
          </p>
          <FormSubmitFooter
            formId={FORM_ID}
            busy={save.isPending}
            invalid={problems.length > 0}
            onCancel={() => onOpenChange(false)}
            submitLabel={t(inspection ? 'inspections.editor.save' : 'inspections.editor.create')}
          />
        </>
      }
    >
      <form
        id={FORM_ID}
        data-inspection-editor={inspection ? 'edit' : 'new'}
        onSubmit={(event) => {
          event.preventDefault();
          // The drawer is drawn outside the page, but its events still rise through the
          // page that opened it: this one is nobody else's.
          event.stopPropagation();
          if (problems.length === 0 && !save.isPending) save.mutate();
        }}
      >
        <FormSection title={t('inspections.editor.what')}>
          <FormField label={t('inspections.editor.name')} required>
            <FormInput
              value={draft.name}
              maxLength={NAME_MAX}
              placeholder={t('inspections.editor.name_placeholder')}
              onChange={(event) => change({ name: event.target.value })}
            />
          </FormField>
          <FormField label={t('inspections.editor.stream')} required>
            <FormSelect
              ariaLabel={t('inspections.editor.stream')}
              value={draft.stream}
              placeholder={t('inspections.editor.stream_placeholder')}
              onChange={(stream) => change({ stream })}
              options={logStreams}
            />
          </FormField>
          <FormField label={t('inspections.editor.statement')} hint={t('inspections.editor.statement_hint')}>
            <FormTextarea
              value={draft.statement}
              rows={3}
              className="min-h-[72px]"
              placeholder={t('inspections.editor.statement_placeholder')}
              onChange={(event) => change({ statement: event.target.value })}
            />
          </FormField>
          <FormField
            label={t('explore.inspection.requirements')}
            hint={t('explore.inspection.requirements_hint')}
          >
            <FormTextarea
              value={draft.requirements}
              rows={4}
              maxLength={REQUIREMENTS_MAX}
              placeholder={t('explore.inspection.requirements_placeholder')}
              onChange={(event) => change({ requirements: event.target.value })}
            />
          </FormField>
        </FormSection>

        <FormSection title={t('inspections.editor.when')}>
          <ScheduleField draft={draft} limits={limits} timezone={timezone} onChange={change} />
          <div className="grid grid-cols-3 gap-3">
            <FormField label={t('inspections.editor.window')} hint={t('inspections.editor.window_hint')}>
              <FormSelect
                ariaLabel={t('inspections.editor.window')}
                value={String(draft.windowSecs)}
                onChange={(value) => {
                  setWindowChosen(true);
                  change({ windowSecs: Number(value) });
                }}
                options={WINDOW_SECONDS.map((seconds) => ({
                  value: String(seconds),
                  label: durationLabel(t, seconds),
                }))}
              />
            </FormField>
            <FormField
              label={t('explore.inspection.model')}
              className="col-span-2"
              hint={
                !model
                  ? t('inspections.editor.no_model')
                  : model.enabled
                    ? t('explore.inspection.disclosure', { model: model.default_model })
                    : t('inspections.editor.model_off_hint')
              }
            >
              <FormSelect
                ariaLabel={t('explore.inspection.model')}
                value={draft.providerId}
                placeholder={t('inspections.editor.model_placeholder')}
                onChange={(providerId) => change({ providerId })}
                options={models.map((provider) => {
                  const name = `${provider.name} · ${provider.default_model}`;

                  // The model of the inspection stays shown after it was disabled, and says so.
                  return {
                    value: provider.id,
                    label: provider.enabled ? name : t('inspections.editor.model_off', { name }),
                  };
                })}
              />
            </FormField>
          </div>
          <label className="flex min-h-9 items-center gap-2 font-sans text-xs text-tx-1">
            <Switch
              checked={draft.enabled}
              onCheckedChange={(enabled) => change({ enabled })}
              aria-label={t('inspections.editor.enabled')}
            />
            <span>{t('inspections.editor.enabled')}</span>
          </label>
        </FormSection>

        <FormSection title={t('inspections.editor.whom')} className="mb-2">
          <FormField label={t('inspections.editor.notify_when')}>
            <FormSelect
              ariaLabel={t('inspections.editor.notify_when')}
              value={draft.notifyWhen}
              onChange={(value) => change({ notifyWhen: value as InspectionDraft['notifyWhen'] })}
              options={NOTIFY_WHEN.map((when) => ({
                value: when,
                label: t(`inspections.notify_when.${when}`),
              }))}
            />
          </FormField>
          <div className="flex flex-col gap-1.5">
            <span className="type-label font-sans font-medium text-tx-2">
              {t('inspections.editor.channels')}
            </span>
            {!mayListConnectors || connectors.isError ? (
              // The channels cannot be shown, so they are left as they were saved, and said to be.
              <p
                data-inspection-channels={mayListConnectors ? 'failed' : 'denied'}
                className="m-0 font-sans text-xs leading-relaxed text-tx-3"
              >
                {t(`inspections.editor.channels_${mayListConnectors ? 'failed' : 'denied'}`)}
                {draft.routes.length > 0 &&
                  ` ${t('inspections.editor.channels_kept', { count: draft.routes.length })}`}
              </p>
            ) : connectors.isPending ? null : channels.length === 0 ? (
              <p data-inspection-channels="none" className="m-0 font-sans text-xs leading-relaxed text-tx-3">
                {t('inspections.editor.channels_none')}
              </p>
            ) : (
              <RoutesField
                routes={draft.routes}
                connectors={channels}
                onChange={(routes) => change({ routes })}
              />
            )}
            <span className="font-sans text-xs leading-relaxed text-tx-3">
              {t('inspections.editor.channels_hint')}
            </span>
          </div>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
