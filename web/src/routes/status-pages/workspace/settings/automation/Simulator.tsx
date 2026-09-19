import { useMutation, useQuery } from '@tanstack/react-query';
import { FlaskConical, Play } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as incidentsApi from '@/api/incidents';
import * as statusPagesApi from '@/api/statusPages';
import type {
  ActiveAutomationRule,
  AlertSeverity,
  AutomationSimulationInput,
  AutomationSourceKind,
} from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, Pill } from '@/shell/chrome';
import { FormField, FormInput, FormRow, FormSelect } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { SourceResourceField } from './SourceResourceField';

export function AutomationSimulator({
  pageId,
  rules,
}: {
  pageId: string;
  rules: ActiveAutomationRule[];
}) {
  const { t } = useTranslation('status-pages');
  const alertRead = useActionAccess({ permission: 'alerts.read' });
  const incidents = useQuery({
    queryKey: ['status-pages', pageId, 'automation', 'simulator-incidents'],
    queryFn: () => incidentsApi.list({ scope: 'active' }),
    enabled: alertRead.allowed,
  });
  const [preset, setPreset] = React.useState('manual');
  const [sourceKind, setSourceKind] = React.useState<AutomationSourceKind>('alert_incident');
  const [sourceId, setSourceId] = React.useState('');
  const [sourceInstanceId, setSourceInstanceId] = React.useState('');
  const [severity, setSeverity] = React.useState<AlertSeverity>('warning');
  const [labels, setLabels] = React.useState('');
  const simulation = useMutation({
    mutationFn: () => statusPagesApi.simulateAutomation(pageId, simulationInput({
      sourceKind,
      sourceId,
      sourceInstanceId,
      severity,
      labels,
    })),
    onError: (error) => toast.error(toApiError(error).message),
  });
  const matchedRule = rules.find((rule) => rule.rule.id === simulation.data?.matched_rule_id);

  const choosePreset = (value: string) => {
    setPreset(value);
    if (value === 'manual') return;
    const incident = incidents.data?.find((item) => item.id === value);
    if (!incident) return;
    setSourceKind('alert_incident');
    setSourceId(incident.rule_id);
    setSourceInstanceId(incident.id);
    setSeverity(incident.severity as AlertSeverity);
    setLabels(Object.entries({
      ...incident.labels,
      incident_summary: incident.summary,
      alert_rule_id: incident.rule_id,
      incident_status: incident.status,
    }).map(([key, item]) => `${key}=${item}`).join(', '));
  };

  return (
    <div className="rounded-md border border-bd-0 bg-bg-2">
      <div className="flex items-start gap-3 border-b border-bd-0 px-4 py-3">
        <FlaskConical className="mt-0.5 h-4 w-4 shrink-0 text-indigo-soft" />
        <div>
          <div className="text-sm font-strong text-tx-0">{t('automation.simulator.title')}</div>
          <div className="mt-1 text-xs leading-relaxed text-tx-3">{t('automation.simulator.description')}</div>
        </div>
      </div>
      <div className="space-y-4 p-4">
        {alertRead.allowed && (
          <FormField label={t('automation.simulator.incident')}>
            <FormSelect
              value={preset}
              onChange={choosePreset}
              options={[
                { value: 'manual', label: t('automation.simulator.manual') },
                ...(incidents.data ?? []).map((incident) => ({
                  value: incident.id,
                  label: incident.summary,
                })),
              ]}
            />
          </FormField>
        )}
        <FormRow className="grid-cols-1 sm:grid-cols-2">
          <FormField label={t('automation.fields.source_kind')}>
            <FormSelect
              value={sourceKind}
              onChange={(value) => {
                setPreset('manual');
                setSourceId('');
                setSourceInstanceId('');
                setSourceKind(value as AutomationSourceKind);
              }}
              options={[
                { value: 'alert_incident', label: t('automation.source.alert_incident') },
                { value: 'synthetic_monitor', label: t('automation.source.synthetic_monitor') },
              ]}
            />
          </FormField>
          <FormField label={t('automation.fields.minimum_severity')}>
            <FormSelect
              value={severity}
              onChange={(value) => setSeverity(value as AlertSeverity)}
              options={(['info', 'warning', 'error', 'critical'] as const).map((item) => ({
                value: item,
                label: t(`automation.severity.${item}`),
              }))}
            />
          </FormField>
        </FormRow>
        <SourceResourceField
          sourceKind={sourceKind}
          value={sourceId}
          allowAll={false}
          required
          onChange={(value) => {
            setPreset('manual');
            setSourceInstanceId('');
            setSourceId(value);
          }}
        />
        <FormField label={t('automation.fields.labels')} hint={t('automation.fields.labels_hint')}>
          <FormInput
            value={labels}
            className="text-base sm:text-sm"
            onChange={(event) => setLabels(event.currentTarget.value)}
          />
        </FormField>
        <div className="flex flex-wrap items-center gap-3 border-t border-bd-0 pt-4">
          <ChromeButton
            size="sm"
            disabled={!sourceId.trim() || simulation.isPending}
            onClick={() => simulation.mutate()}
          >
            <Play className="h-3.5 w-3.5" />
            {t('automation.actions.simulate')}
          </ChromeButton>
          {simulation.data && (
            <div className="flex min-w-0 items-center gap-2 text-xs text-tx-2">
              <Pill tone={matchedRule ? 'green' : 'dim'}>
                {matchedRule ? t('automation.simulator.matched') : t('automation.simulator.no_match')}
              </Pill>
              {matchedRule && <span className="truncate font-strong text-tx-0">{matchedRule.rule.name}</span>}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function simulationInput({
  sourceKind,
  sourceId,
  sourceInstanceId,
  severity,
  labels,
}: {
  sourceKind: AutomationSourceKind;
  sourceId: string;
  sourceInstanceId: string;
  severity: AlertSeverity;
  labels: string;
}): AutomationSimulationInput {
  return {
    source_kind: sourceKind,
    source_id: sourceId.trim(),
    ...(sourceInstanceId.trim() ? { source_instance_id: sourceInstanceId.trim() } : {}),
    severity,
    labels: Object.fromEntries(labels.split(',').map((item) => item.trim()).filter(Boolean).map((item) => {
      const [key = '', ...value] = item.split('=');
      return [key.trim(), value.join('=').trim()];
    }).filter(([key, value]) => key && value)),
  };
}
