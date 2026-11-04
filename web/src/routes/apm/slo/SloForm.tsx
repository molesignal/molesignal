import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { apmApi, apmQueryKeys, type TransactionIdentity } from '@/api/apm';
import { type ServiceLevelObjective, SLO_WINDOW_DAYS, sloApi, type SloKind, sloQueryKeys, type SloWrite } from '@/api/apmSlo';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { Button } from '@/shell/ui/button';
import { Input } from '@/shell/ui/input';
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/shell/ui/sheet';
import { toast } from '@/shell/ui/sonner';
import { Textarea } from '@/shell/ui/textarea';
import { useAuthStore } from '@/stores/auth';

import { Choice, Field } from '../catalog/DefinitionDrawer';

const DAY_MICROS = 86_400_000_000;
const SEPARATOR = '\u241f';

/** What the form opens with: an objective to change, or a service to set a new one on. */
export interface SloFormTarget {
  slo: ServiceLevelObjective | null;
  service?: { namespace: string; name: string; environment: string } | null;
}

/**
 * Sets a service level objective: the service in an environment, the whole
 * of it or one endpoint, what makes a request good (it did not fail, or it
 * did not fail and was quick enough), the share promised and the window.
 */
export function SloFormDrawer({ target, open, onClose }: { target: SloFormTarget | null; open: boolean; onClose: () => void }) {
  const { t } = useTranslation('apm');

  return (
    <Sheet open={open} onOpenChange={(next) => !next && onClose()}>
      <SheetContent side="right" className="flex w-[460px] max-w-full flex-col gap-0 p-0" data-apm-slo-form="">
        <div className="border-b border-bd-0 px-4 py-3">
          <SheetTitle className="font-sans text-sm font-strong text-tx-0">{target?.slo ? t('slo.form.edit_title') : t('slo.form.new_title')}</SheetTitle>
          <SheetDescription className="mt-0.5 font-sans text-xs text-tx-2">{t('slo.form.description')}</SheetDescription>
        </div>
        {/* One form per opening: its fields start from the objective each time. */}
        {open && <SloForm key={target?.slo?.id ?? 'new'} target={target} onDone={onClose} />}
      </SheetContent>
    </Sheet>
  );
}

function SloForm({ target, onDone }: { target: SloFormTarget | null; onDone: () => void }) {
  const { t } = useTranslation('apm');
  const queryClient = useQueryClient();
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const access = useActionAccess({ permission: 'apm.slos.manage' });
  const existing = target?.slo ?? null;
  const preset = existing ?? null;
  const [range] = React.useState(() => {
    const to = Date.now() * 1_000;
    return { from: to - DAY_MICROS, to };
  });

  const [name, setName] = React.useState(preset?.name ?? '');
  const [description, setDescription] = React.useState(preset?.description ?? '');
  const [namespace, setNamespace] = React.useState(preset?.namespace ?? target?.service?.namespace ?? '');
  const [service, setService] = React.useState(preset?.service ?? target?.service?.name ?? '');
  const [environment, setEnvironment] = React.useState(preset?.environment ?? target?.service?.environment ?? '');
  const [endpoint, setEndpoint] = React.useState(preset?.transaction ? key(preset.transaction) : '');
  const [kind, setKind] = React.useState<SloKind>(preset?.kind ?? 'availability');
  const [thresholdMs, setThresholdMs] = React.useState(String(preset?.latencyThreshold ? preset.latencyThreshold / 1_000 : 300));
  const [objective, setObjective] = React.useState(String(preset ? +(preset.objective * 100).toFixed(4) : 99.9));
  const [windowDays, setWindowDays] = React.useState(String(preset?.windowDays ?? 30));

  const servicesParams = { ...range, limit: 200, sort: 'request_count', direction: 'desc' as const };
  const services = useQuery({
    queryKey: apmQueryKeys.services(orgId, servicesParams),
    queryFn: () => apmApi.services(servicesParams),
    enabled: Boolean(orgId),
  });
  const detailParams = { ...range, ...(namespace ? { namespace } : {}), ...(environment ? { environment } : {}) };
  const detail = useQuery({
    queryKey: apmQueryKeys.service(orgId, service, detailParams),
    queryFn: () => apmApi.service(service, detailParams),
    enabled: Boolean(orgId && service && environment),
  });

  const listed = services.data?.items ?? [];
  const serviceOptions = unique([
    ...(service ? [{ value: `${namespace}${SEPARATOR}${service}`, label: service }] : []),
    ...listed.map((item) => ({ value: `${item.service.namespace}${SEPARATOR}${item.service.name}`, label: item.service.name })),
  ]);
  const environmentOptions = unique([
    ...(environment ? [{ value: environment, label: environment }] : []),
    ...listed
      .filter((item) => item.service.name === service && item.service.namespace === namespace)
      .map((item) => ({ value: item.service.environment, label: item.service.environment })),
  ]);
  const endpointOptions = unique([
    ...(preset?.transaction ? [{ value: key(preset.transaction), label: preset.transaction.name }] : []),
    ...(detail.data?.transactions ?? []).map((item) => ({ value: key(item.transaction), label: item.transaction.name })),
  ]);

  const objectiveValue = Number(objective) / 100;
  const thresholdMicros = Math.round(Number(thresholdMs) * 1_000);
  const ready =
    name.trim() !== '' &&
    service !== '' &&
    environment !== '' &&
    objectiveValue > 0 &&
    objectiveValue < 1 &&
    (kind === 'availability' || thresholdMicros > 0);

  const save = useMutation({
    mutationFn: () => {
      const write: SloWrite = {
        name: name.trim(),
        description: description.trim(),
        namespace,
        service,
        environment,
        transaction: endpoint ? fromKey(endpoint) : null,
        kind,
        latencyThreshold: kind === 'latency' ? thresholdMicros : null,
        objective: objectiveValue,
        windowDays: Number(windowDays),
      };
      return existing ? sloApi.update(existing.id, write) : sloApi.create(write);
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: sloQueryKeys.all });
      toast.success(t('slo.form.saved'));
      onDone();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <form
      className="flex min-h-0 flex-1 flex-col"
      onSubmit={(event) => {
        event.preventDefault();
        if (ready) save.mutate();
      }}
    >
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-4 py-4 font-sans text-xs">
        <Field label={t('slo.form.name')}>
          <Input value={name} onChange={(event) => setName(event.target.value)} required maxLength={255} data-apm-slo-name="" />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label={t('slo.form.service')}>
            <Choice
              value={service ? `${namespace}${SEPARATOR}${service}` : ''}
              onChange={(value) => {
                const [nextNamespace = '', nextService = ''] = value.split(SEPARATOR);
                setNamespace(nextNamespace);
                setService(nextService);
                setEnvironment(listed.find((item) => item.service.name === nextService && item.service.namespace === nextNamespace)?.service.environment ?? '');
                setEndpoint('');
              }}
              none={t('slo.form.choose')}
              options={serviceOptions}
            />
          </Field>
          <Field label={t('slo.form.environment')}>
            {/* Mounted again for each service, so the environment it brings arrives with its option. */}
            <Choice
              key={`${namespace}${SEPARATOR}${service}`}
              value={environment}
              onChange={(value) => {
                setEnvironment(value);
                setEndpoint('');
              }}
              none={t('slo.form.choose')}
              options={environmentOptions}
            />
          </Field>
        </div>
        <Field label={t('slo.form.endpoint')} hint={t('slo.form.endpoint_hint')}>
          <Choice value={endpoint} onChange={setEndpoint} none={t('slo.form.whole_service')} options={endpointOptions} />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label={t('slo.form.kind')}>
            <Choice
              value={kind === 'availability' ? '' : kind}
              onChange={(value) => setKind((value || 'availability') as SloKind)}
              none={t('slo.kind.availability')}
              options={[{ value: 'latency', label: t('slo.kind.latency') }]}
            />
          </Field>
          {kind === 'latency' && (
            <Field label={t('slo.form.threshold')}>
              <Input type="number" min={1} step="any" value={thresholdMs} onChange={(event) => setThresholdMs(event.target.value)} />
            </Field>
          )}
        </div>
        <div className="grid grid-cols-2 gap-3">
          <Field label={t('slo.form.objective')} hint={t('slo.form.objective_hint')}>
            <Input type="number" min={1} max={99.999} step="any" value={objective} onChange={(event) => setObjective(event.target.value)} data-apm-slo-objective="" />
          </Field>
          <Field label={t('slo.form.window')}>
            <Choice
              value={windowDays === '30' ? '' : windowDays}
              onChange={(value) => setWindowDays(value || '30')}
              none={t('slo.window', { count: 30 })}
              options={SLO_WINDOW_DAYS.filter((days) => days !== 30).map((days) => ({ value: String(days), label: t('slo.window', { count: days }) }))}
            />
          </Field>
        </div>
        <Field label={t('slo.form.notes')}>
          <Textarea value={description} onChange={(event) => setDescription(event.target.value)} rows={3} maxLength={2000} />
        </Field>
      </div>
      <div className="flex items-center gap-2 border-t border-bd-0 px-4 py-3">
        <div className="flex-1" />
        <Button type="button" variant="outline" size="sm" onClick={onDone}>
          {t('actions.cancel')}
        </Button>
        <Button type="submit" size="sm" disabled={access.disabled || save.isPending || !ready} title={access.reason} data-apm-slo-save="">
          {t('actions.save')}
        </Button>
      </div>
    </form>
  );
}

function key(transaction: TransactionIdentity): string {
  return `${transaction.kind}${SEPARATOR}${transaction.name}`;
}

function fromKey(value: string): TransactionIdentity {
  const [kind = 'http', name = ''] = value.split(SEPARATOR);
  return { name, kind: kind as TransactionIdentity['kind'] };
}

function unique<T extends { value: string }>(options: readonly T[]): T[] {
  const seen = new Set<string>();
  return options.filter((option) => {
    if (seen.has(option.value)) return false;
    seen.add(option.value);
    return true;
  });
}
