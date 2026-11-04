import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as alertSourcesApi from '@/api/alertSources';
import {
  ALERT_SOURCE_KINDS,
  type AlertSource,
  type AlertSourceKind,
  type EmailRules,
  type IssuedAlertSource,
} from '@/api/alertSources';
import * as escalationsApi from '@/api/escalations';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ReadOnlyValue } from '@/routes/settings/_atoms';
import {
  FieldArray,
  FormDrawer,
  FormField,
  FormInput,
  FormRadio,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import type { Severity } from '@/types/alerting';

import { labelsFromPairs } from './model';

export const ALERT_SOURCES_KEY = ['alert-sources'] as const;

const SEVERITIES: readonly Severity[] = ['critical', 'error', 'warning', 'info'];

const NO_EMAIL_RULES: EmailRules = { allowedSenders: [], resolvePattern: '', ignorePattern: '', dedupPattern: '' };

/** The escalation a new source takes: the `default` policy, the same as a rule saved in the workbench. */
const DEFAULT_POLICY = 'default';

interface LabelPair {
  key: string;
  value: string;
}

/**
 * The form of one alert source: which sender it reads, and how the incidents
 * it opens are escalated, how bad they are when the sender does not say, and
 * what they are labelled. Its kind is chosen once; a saved source keeps it.
 */
export function SourceDrawer({
  open,
  editing,
  onClose,
  onCreated,
}: {
  open: boolean;
  editing: AlertSource | null;
  onClose: () => void;
  /** A new source, with the token that is shown once. */
  onCreated: (source: IssuedAlertSource) => void;
}) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.sources.manage' });
  const [kind, setKind] = React.useState<AlertSourceKind>('alertmanager');
  const [name, setName] = React.useState('');
  const [enabled, setEnabled] = React.useState(true);
  const [policy, setPolicy] = React.useState(DEFAULT_POLICY);
  const [severity, setSeverity] = React.useState<Severity>('warning');
  const [labels, setLabels] = React.useState<LabelPair[]>([]);
  const [homeCluster, setHomeCluster] = React.useState('');
  const [emailRules, setEmailRules] = React.useState<EmailRules>(NO_EMAIL_RULES);
  const [senders, setSenders] = React.useState('');

  const policiesQuery = useQuery({
    queryKey: ['escalation-policies'],
    queryFn: () => escalationsApi.list(),
    enabled: open,
  });
  const clustersQuery = useQuery({
    queryKey: [...ALERT_SOURCES_KEY, 'clusters'],
    queryFn: () => alertSourcesApi.clusters(),
    enabled: open,
    staleTime: 60_000,
  });
  const clusters = clustersQuery.data?.clusters ?? [];

  React.useEffect(() => {
    if (!open) return;

    setKind(editing?.kind ?? 'alertmanager');
    setName(editing?.name ?? '');
    setEnabled(editing?.enabled ?? true);
    setPolicy(editing ? (editing.escalationPolicyId ?? '') : DEFAULT_POLICY);
    setSeverity(editing?.severity ?? 'warning');
    setLabels(Object.entries(editing?.labels ?? {}).map(([key, value]) => ({ key, value })));
    setHomeCluster(editing?.homeCluster ?? '');
    setEmailRules(editing?.email ?? NO_EMAIL_RULES);
    setSenders((editing?.email?.allowedSenders ?? []).join(', '));
  }, [open, editing]);

  const save = useMutation({
    mutationFn: async () => {
      const input: alertSourcesApi.AlertSourceInput = {
        name: name.trim(),
        enabled,
        escalationPolicyId: policy || null,
        severity,
        labels: labelsFromPairs(labels),
        ...(homeCluster ? { homeCluster: homeCluster } : {}),
        ...(kind === 'email'
          ? {
              email: {
                ...emailRules,
                allowedSenders: senders
                  .split(',')
                  .map((sender) => sender.trim())
                  .filter(Boolean),
              },
            }
          : {}),
      };

      if (editing) return { saved: await alertSourcesApi.update(editing.id, input), issued: null };
      return { saved: null, issued: await alertSourcesApi.create({ ...input, kind }) };
    },
    onSuccess: ({ issued }) => {
      void queryClient.invalidateQueries({ queryKey: ALERT_SOURCES_KEY });

      if (issued) {
        toast.success(t('alert_sources.toast_created'));
        onCreated(issued);
      } else {
        toast.success(t('alert_sources.toast_updated'));
      }

      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const submit = (event: React.FormEvent) => {
    event.preventDefault();

    if (!manageAccess.allowed) return;
    if (!name.trim()) {
      toast.error(t('alert_sources.errors.name_required'));
      return;
    }

    save.mutate();
  };

  const policies = policiesQuery.data ?? [];
  const policyOptions = [
    { value: DEFAULT_POLICY, label: t('alert_sources.policy_default') },
    ...policies
      .filter((candidate) => candidate.name !== DEFAULT_POLICY)
      .map((candidate) => ({ value: candidate.id, label: candidate.name })),
    { value: '', label: t('alert_sources.policy_none') },
  ];

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={editing ? t('alert_sources.edit_title') : t('alert_sources.drawer_title')}
      subtitle={t('alert_sources.drawer_subtitle')}
      width={760}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          invalid={!name.trim()}
          onCancel={onClose}
          submitLabel={editing ? t('alert_sources.save') : t('alert_sources.create')}
          formId="alert-source-form"
        />
      }
    >
      <form id="alert-source-form" onSubmit={submit}>
        <fieldset
          disabled={manageAccess.disabled}
          aria-disabled={manageAccess.disabled || undefined}
          className="contents"
        >
          <FormSection title={t('alert_sources.sections.kind')}>
            {editing ? (
              <ReadOnlyValue value={t(`alert_sources.kinds.${editing.kind}`)} />
            ) : (
              <FormRadio
                value={kind}
                onChange={setKind}
                className="grid grid-cols-2 gap-1.5"
                options={ALERT_SOURCE_KINDS.map((option) => ({
                  value: option,
                  label: t(`alert_sources.kinds.${option}`),
                  hint: t(`alert_sources.kind_hints.${option}`),
                }))}
              />
            )}
          </FormSection>

          <FormSection title={t('alert_sources.sections.identity')}>
            <FormField label={t('alert_sources.fields.name')} required>
              <FormInput
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder={t('alert_sources.fields.name_placeholder')}
                required
              />
            </FormField>
            <label className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0">
              <Checkbox checked={enabled} onCheckedChange={(next) => setEnabled(next === true)} />
              <span>{t('alert_sources.fields.enabled')}</span>
            </label>
          </FormSection>

          <FormSection title={t('alert_sources.sections.incidents')}>
            <FormField label={t('alert_sources.fields.policy')}>
              <FormSelect value={policy} onChange={setPolicy} options={policyOptions} />
            </FormField>
            <FormField label={t('alert_sources.fields.severity')} hint={t('alert_sources.fields.severity_hint')}>
              <FormSelect
                value={severity}
                onChange={(value) => setSeverity(value as Severity)}
                options={SEVERITIES.map((value) => ({ value, label: t(`severity.${value}`) }))}
              />
            </FormField>
            {clusters.length > 0 && (
              <FormField label={t('alert_sources.fields.cluster')} hint={t('alert_sources.fields.cluster_hint')}>
                <FormSelect
                  value={homeCluster || (clustersQuery.data?.local ?? '')}
                  onChange={setHomeCluster}
                  options={clusters.map((cluster) => ({
                    value: cluster.id,
                    label: cluster.local
                      ? t('alert_sources.fields.cluster_local', { name: cluster.name })
                      : cluster.name,
                  }))}
                />
              </FormField>
            )}
          </FormSection>

          {kind === 'email' && (
            <FormSection
              title={t('alert_sources.sections.email')}
              description={t('alert_sources.email.description')}
            >
              <FormField label={t('alert_sources.email.senders')} hint={t('alert_sources.email.senders_hint')}>
                <FormInput
                  value={senders}
                  onChange={(event) => setSenders(event.target.value)}
                  placeholder="nagios@example.com, @monitoring.example.com"
                />
              </FormField>
              <FormField label={t('alert_sources.email.resolve')} hint={t('alert_sources.email.resolve_hint')}>
                <FormInput
                  className="font-mono"
                  value={emailRules.resolvePattern}
                  onChange={(event) => setEmailRules({ ...emailRules, resolvePattern: event.target.value })}
                />
              </FormField>
              <FormField label={t('alert_sources.email.dedup')} hint={t('alert_sources.email.dedup_hint')}>
                <FormInput
                  className="font-mono"
                  value={emailRules.dedupPattern}
                  onChange={(event) => setEmailRules({ ...emailRules, dedupPattern: event.target.value })}
                  placeholder="host=(\S+)"
                />
              </FormField>
              <FormField label={t('alert_sources.email.ignore')} hint={t('alert_sources.email.ignore_hint')}>
                <FormInput
                  className="font-mono"
                  value={emailRules.ignorePattern}
                  onChange={(event) => setEmailRules({ ...emailRules, ignorePattern: event.target.value })}
                />
              </FormField>
            </FormSection>
          )}

          <FormSection
            title={t('alert_sources.sections.labels')}
            description={t('alert_sources.fields.labels_hint')}
            className="mb-0"
          >
            <FieldArray
              items={labels}
              onChange={setLabels}
              newItem={() => ({ key: '', value: '' })}
              addLabel={t('alert_sources.fields.add_label')}
              removeLabel={t('alert_sources.fields.remove_label')}
              maxItems={32}
              renderItem={(pair, _index, setPair) => (
                <div className="grid grid-cols-2 gap-2">
                  <FormInput
                    aria-label={t('alert_sources.fields.label_key')}
                    value={pair.key}
                    onChange={(event) => setPair({ ...pair, key: event.target.value })}
                    placeholder="team"
                  />
                  <FormInput
                    aria-label={t('alert_sources.fields.label_value')}
                    value={pair.value}
                    onChange={(event) => setPair({ ...pair, value: event.target.value })}
                    placeholder="payments"
                  />
                </div>
              )}
            />
          </FormSection>
        </fieldset>
      </form>
    </FormDrawer>
  );
}
