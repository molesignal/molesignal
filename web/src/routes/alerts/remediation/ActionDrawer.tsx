import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as remediationApi from '@/api/remediation';
import type { KubernetesCluster, RemediationAction } from '@/api/remediation';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { uiLabelClass } from '@/shell/chrome';
import {
  FieldArray,
  FormDrawer,
  FormField,
  FormInput,
  FormRadio,
  FormSection,
  FormSelect,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { JsonEditor } from '@/shell/JsonEditor';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';

import { ACTIONS_KEY, type ActionDraft, bodyReadsAsJson, draftOf, type HeaderRow, inputOf, type ParameterRow } from './model';

/** One remediation action: how it runs, what is asked when it is started, which incidents it is for, how it is checked. */
export function RemediationActionDrawer({
  open,
  editing,
  clusters,
  onClose,
}: {
  open: boolean;
  editing: RemediationAction | null;
  clusters: KubernetesCluster[];
  onClose: () => void;
}) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.remediation.manage' });
  const [draft, setDraft] = React.useState<ActionDraft>(() => draftOf(editing));
  const set = (patch: Partial<ActionDraft>) => setDraft((current) => ({ ...current, ...patch }));

  React.useEffect(() => {
    if (open) setDraft(draftOf(editing));
  }, [editing, open]);

  const save = useMutation({
    mutationFn: () => {
      const input = inputOf(draft, editing?.teamIds ?? []);
      return editing ? remediationApi.updateAction(editing.id, input) : remediationApi.createAction(input);
    },
    onSuccess: (saved) => {
      toast.success(t('remediation.saved', { name: saved.name }));
      void queryClient.invalidateQueries({ queryKey: ACTIONS_KEY });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const secretSet = editing?.secretsSet.includes('signingSecret') ?? false;
  const headerSet = (name: string) => editing?.secretsSet.includes(`header:${name}`) ?? false;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={editing ? t('remediation.drawer.edit_title') : t('remediation.drawer.new_title')}
      subtitle={t('remediation.drawer.subtitle')}
      footer={
        <FormSubmitFooter
          formId="remediation-action-form"
          busy={save.isPending}
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          onCancel={onClose}
          submitLabel={t('remediation.drawer.save')}
        />
      }
    >
      <form
        id="remediation-action-form"
        data-remediation-drawer
        onSubmit={(event) => {
          event.preventDefault();
          save.mutate();
        }}
      >
        <FormSection title={t('remediation.drawer.identity')}>
          <FormField label={t('remediation.drawer.name')} required>
            <FormInput value={draft.name} onChange={(event) => set({ name: event.target.value })} required />
          </FormField>
          <FormField label={t('remediation.drawer.description')}>
            <FormTextarea value={draft.description} rows={2} onChange={(event) => set({ description: event.target.value })} />
          </FormField>
        </FormSection>

        <FormSection title={t('remediation.drawer.target')}>
          <FormRadio
            value={draft.kind}
            onChange={(kind) => set({ kind })}
            options={[
              { value: 'http', label: t('remediation.kinds.http'), hint: t('remediation.kinds.http_hint') },
              { value: 'kubernetes', label: t('remediation.kinds.kubernetes'), hint: t('remediation.kinds.kubernetes_hint') },
            ]}
          />
          {draft.kind === 'http' ? (
            <>
              <div className="grid grid-cols-[110px_minmax(0,1fr)] gap-2">
                <FormField label={t('remediation.drawer.method')}>
                  <FormSelect
                    value={draft.method}
                    onChange={(method) => set({ method: method as ActionDraft['method'] })}
                    options={['POST', 'PUT', 'PATCH', 'GET']}
                  />
                </FormField>
                <FormField label={t('remediation.drawer.url')} required>
                  <FormInput
                    value={draft.url}
                    onChange={(event) => set({ url: event.target.value })}
                    placeholder="https://ci.example.com/job/restart/buildWithParameters?service={{incident.service}}"
                    className="font-mono"
                  />
                </FormField>
              </div>
              {/* Not a FormField: its label would name the buttons inside. */}
              <div className="flex flex-col gap-1.5">
                <span className={uiLabelClass}>{t('remediation.drawer.headers')}</span>
                <span className="text-xs text-tx-3">{t('remediation.drawer.headers_hint')}</span>
                <FieldArray<HeaderRow>
                  items={draft.headers}
                  onChange={(headers) => set({ headers })}
                  newItem={() => ({ name: '', value: '' })}
                  addLabel={t('remediation.drawer.add_header')}
                  removeLabel={t('remediation.drawer.remove_header')}
                  maxItems={10}
                  renderItem={(row, _index, setRow) => (
                    <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)] gap-2">
                      <FormInput
                        aria-label={t('remediation.drawer.header_name')}
                        value={row.name}
                        onChange={(event) => setRow({ ...row, name: event.target.value })}
                        placeholder="Authorization"
                      />
                      <FormInput
                        aria-label={t('remediation.drawer.header_value')}
                        type="password"
                        value={row.value}
                        onChange={(event) => setRow({ ...row, value: event.target.value })}
                        placeholder={headerSet(row.name) ? t('remediation.drawer.secret_kept') : ''}
                      />
                    </div>
                  )}
                />
              </div>
              <FormField label={t('remediation.drawer.body')} hint={t('remediation.drawer.body_hint')}>
                <JsonEditor
                  value={draft.body}
                  onChange={(body) => set({ body })}
                  ariaLabel={t('remediation.drawer.body')}
                  minHeight={112}
                  placeholder={'{"service": "{{incident.service}}", "incident": "{{incident.url}}"}'}
                  // A placeholder used bare as a number is no mistake: problems show only when the filled-in body is not JSON.
                  showProblems={!bodyReadsAsJson(draft.body)}
                />
              </FormField>
              <FormField label={t('remediation.drawer.signing_secret')} hint={t('remediation.drawer.signing_secret_hint')}>
                <FormInput
                  type="password"
                  value={draft.signingSecret}
                  onChange={(event) => set({ signingSecret: event.target.value, clearSecret: false })}
                  placeholder={secretSet ? t('remediation.drawer.secret_kept') : ''}
                />
              </FormField>
            </>
          ) : (
            <>
              <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                <FormField label={t('remediation.drawer.cluster')} required>
                  <FormSelect
                    value={draft.clusterId}
                    onChange={(clusterId) => set({ clusterId })}
                    options={[
                      { value: '', label: t('remediation.drawer.pick_cluster') },
                      ...clusters.map((cluster) => ({ value: cluster.id, label: cluster.name })),
                    ]}
                  />
                </FormField>
                <FormField label={t('remediation.drawer.operation')}>
                  <FormSelect
                    value={draft.operation}
                    onChange={(operation) => set({ operation: operation as ActionDraft['operation'] })}
                    options={(['restart', 'scale', 'rollback'] as const).map((operation) => ({
                      value: operation,
                      label: t(`remediation.operations.${operation}`),
                    }))}
                  />
                </FormField>
                <FormField label={t('remediation.drawer.workload_kind')}>
                  <FormSelect
                    value={draft.workloadKind}
                    onChange={(workloadKind) => set({ workloadKind: workloadKind as ActionDraft['workloadKind'] })}
                    options={['Deployment', 'StatefulSet', 'DaemonSet']}
                  />
                </FormField>
                <FormField label={t('remediation.drawer.namespace')} required>
                  <FormInput value={draft.namespace} onChange={(event) => set({ namespace: event.target.value })} className="font-mono" />
                </FormField>
                <FormField label={t('remediation.drawer.workload_name')} required>
                  <FormInput value={draft.workloadName} onChange={(event) => set({ workloadName: event.target.value })} className="font-mono" />
                </FormField>
                {draft.operation === 'scale' && (
                  <FormField label={t('remediation.drawer.replicas')} required>
                    <FormInput value={draft.replicas} onChange={(event) => set({ replicas: event.target.value })} className="font-mono" />
                  </FormField>
                )}
              </div>
            </>
          )}
        </FormSection>

        <FormSection title={t('remediation.drawer.parameters')} description={t('remediation.drawer.parameters_hint')}>
          <FieldArray<ParameterRow>
            items={draft.parameters}
            onChange={(parameters) => set({ parameters })}
            newItem={() => ({ key: '', label: '', kind: 'text', options: '', required: false, default: '' })}
            addLabel={t('remediation.drawer.add_parameter')}
            removeLabel={t('remediation.drawer.remove_parameter')}
            maxItems={10}
            renderItem={(row, _index, setRow) => (
              <div className="grid grid-cols-2 gap-2 sm:grid-cols-[1fr_1fr_110px_1fr_auto]">
                <FormInput
                  aria-label={t('remediation.drawer.parameter_key')}
                  value={row.key}
                  onChange={(event) => setRow({ ...row, key: event.target.value })}
                  placeholder="replicas"
                  className="font-mono"
                />
                <FormInput
                  aria-label={t('remediation.drawer.parameter_label')}
                  value={row.label}
                  onChange={(event) => setRow({ ...row, label: event.target.value })}
                  placeholder={t('remediation.drawer.parameter_label')}
                />
                <FormSelect
                  ariaLabel={t('remediation.drawer.parameter_kind')}
                  value={row.kind}
                  onChange={(kind) => setRow({ ...row, kind: kind as ParameterRow['kind'] })}
                  options={(['text', 'number', 'select'] as const).map((kind) => ({
                    value: kind,
                    label: t(`remediation.parameter_kinds.${kind}`),
                  }))}
                />
                <FormInput
                  aria-label={row.kind === 'select' ? t('remediation.drawer.parameter_options') : t('remediation.drawer.parameter_default')}
                  value={row.kind === 'select' ? row.options : row.default}
                  onChange={(event) =>
                    setRow(row.kind === 'select' ? { ...row, options: event.target.value } : { ...row, default: event.target.value })
                  }
                  placeholder={row.kind === 'select' ? 'eu, us' : t('remediation.drawer.parameter_default')}
                />
                <label className="flex items-center gap-1.5 text-xs text-tx-1">
                  <Checkbox checked={row.required} onCheckedChange={(next) => setRow({ ...row, required: next === true })} />
                  {t('remediation.drawer.parameter_required')}
                </label>
              </div>
            )}
          />
        </FormSection>

        <FormSection title={t('remediation.drawer.scope')}>
          <FormField label={t('remediation.drawer.services')} hint={t('remediation.drawer.services_hint')}>
            <FormInput value={draft.services} onChange={(event) => set({ services: event.target.value })} placeholder="checkout, payments" />
          </FormField>
          <div className="flex flex-col gap-1.5">
            <span className={uiLabelClass}>{t('remediation.drawer.risk')}</span>
            <FormRadio
              value={draft.risk}
              onChange={(risk) => set({ risk })}
              options={[
                { value: 'l2', label: t('remediation.risks.l2') },
                { value: 'l3', label: t('remediation.risks.l3') },
              ]}
            />
          </div>
          <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
            <FormField label={t('remediation.drawer.verify_after')} hint={t('remediation.drawer.verify_after_hint')}>
              <div className="flex items-center gap-2">
                <FormInput
                  type="number"
                  min={0}
                  value={String(draft.verifyMinutes)}
                  onChange={(event) => set({ verifyMinutes: Math.max(0, Number(event.target.value) || 0) })}
                />
                <span className="shrink-0 text-xs text-tx-2">{t('workbench.threshold.minutes')}</span>
              </div>
            </FormField>
            <FormField label={t('remediation.drawer.verify_by')}>
              <FormSelect
                value={draft.verifyBy}
                onChange={(verifyBy) => set({ verifyBy: verifyBy as ActionDraft['verifyBy'] })}
                options={(['incident_resolved', 'rule_healthy'] as const).map((by) => ({
                  value: by,
                  label: t(`remediation.verify_by.${by}`),
                }))}
              />
            </FormField>
          </div>
          <label className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0">
            <Checkbox checked={draft.enabled} onCheckedChange={(next) => set({ enabled: next === true })} />
            <span>{t('remediation.drawer.enabled')}</span>
          </label>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
