import { useMutation, useQueryClient } from '@tanstack/react-query';
import { LoaderCircle, Pause, Play } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as alertsApi from '@/api/alerts';
import type { NodeAgentAlertStatus, NodeAgentAssets } from '@/api/nodeAgent';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, Pill, type PillTone } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';
import type { Severity } from '@/types/alerting';

import { NODE_AGENT_ASSETS_KEY } from './model';

const SEVERITY_TONE: Record<Severity, PillTone> = {
  critical: 'red',
  error: 'orange',
  warning: 'yellow',
  info: 'neutral',
};

/**
 * The real-time alert rules of the host events: one event in five minutes opens an incident. The
 * rules are made disabled by the install button; each is switched on or off here with one click,
 * and changed in the alert module like any other.
 */
export function AlertTemplates({ alerts }: { alerts: readonly NodeAgentAlertStatus[] }) {
  const { t } = useTranslation('onboarding');

  return (
    <div data-node-agent-alerts="" className="min-w-0">
      <h4 className="m-0 font-sans text-md font-strong text-tx-0">{t('datasource_page.node_agent.alerts.title')}</h4>
      <p className="m-0 mt-1 max-w-[680px] font-sans text-sm leading-relaxed text-tx-2">
        {t('datasource_page.node_agent.alerts.description')}
      </p>
      <ul className="m-0 mt-[10px] flex list-none flex-col divide-y divide-bd-0 p-0">
        {alerts.map((alert) => (
          <AlertLine key={alert.key} alert={alert} />
        ))}
      </ul>
    </div>
  );
}

function AlertLine({ alert }: { alert: NodeAgentAlertStatus }) {
  const { t } = useTranslation('onboarding');
  const { t: ta } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'alerts.rules.manage' });
  const base = `datasource_page.node_agent.alerts.templates.${alert.key}`;

  const toggle = useMutation({
    mutationFn: async (enabled: boolean) => {
      const rule = await alertsApi.get(alert.ruleId!);

      return alertsApi.update(rule.id, { ...rule, enabled } as alertsApi.AlertRuleInput);
    },
    onSuccess: (rule) => {
      queryClient.setQueryData<NodeAgentAssets>(NODE_AGENT_ASSETS_KEY, (current) =>
        current && {
          ...current,
          alerts: (current.alerts ?? []).map((entry) =>
            entry.key === alert.key ? { ...entry, enabled: rule.enabled } : entry,
          ),
        },
      );
      void queryClient.invalidateQueries({ queryKey: ['alerts', 'rules'] });
      toast.success(t(rule.enabled ? 'datasource_page.node_agent.alerts.enabled_toast' : 'datasource_page.node_agent.alerts.disabled_toast', { name: t(`${base}.name`) }));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <li data-node-agent-alert={alert.key} data-enabled={alert.enabled} className="flex min-w-0 items-center gap-3 py-[10px]">
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-sans text-sm font-strong text-tx-0">{t(`${base}.name`)}</span>
          <Pill tone={SEVERITY_TONE[alert.severity]}>{ta(`severity.${alert.severity}`)}</Pill>
          {alert.enabled && <Pill tone="green">{t('datasource_page.node_agent.alerts.on')}</Pill>}
        </div>
        <p className="m-0 mt-0.5 font-sans text-xs leading-relaxed text-tx-3">{t(`${base}.description`)}</p>
      </div>
      {alert.ruleId ? (
        <div className="flex shrink-0 items-center gap-2">
          <Link
            to={`/alerts/rules/${encodeURIComponent(alert.ruleId)}/edit`}
            className="font-sans text-xs text-tx-2 outline-none transition-colors hover:text-tx-0 focus-visible:text-tx-0 focus-visible:underline"
          >
            {t('datasource_page.node_agent.alerts.edit')}
          </Link>
          <ChromeButton
            size="sm"
            variant={alert.enabled ? 'default' : 'primary'}
            disabled={toggle.isPending || access.disabled}
            disabledReason={access.reason}
            onClick={() => toggle.mutate(!alert.enabled)}
          >
            {toggle.isPending ? (
              <LoaderCircle aria-hidden className="h-3.5 w-3.5 animate-spin" />
            ) : alert.enabled ? (
              <Pause aria-hidden className="h-3.5 w-3.5" />
            ) : (
              <Play aria-hidden className="h-3.5 w-3.5" />
            )}
            {t(alert.enabled ? 'datasource_page.node_agent.alerts.disable' : 'datasource_page.node_agent.alerts.enable')}
          </ChromeButton>
        </div>
      ) : (
        <span className="shrink-0 font-sans text-xs text-tx-3">{t('datasource_page.node_agent.alerts.not_installed')}</span>
      )}
    </li>
  );
}
