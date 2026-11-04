import { Bot } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';


import type { Finding } from '@/api/discovery';
import { sourceOf } from '@/api/discovery';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, Pill } from '@/shell/chrome';
import { FormDrawer, FormSection } from '@/shell/FormDrawer';
import { SEVERITY_TONE } from '@/shell/incident/tones';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';

import { findingHref, findingSummary, formatFindingValue, investigatePrompt } from './model';

/** One finding: what it compares, the changes near when it began, and where to look into it. */
export function FindingDrawer({
  finding,
  onClose,
  onRule,
}: {
  finding: Finding | null;
  onClose: () => void;
  onRule: (finding: Finding) => void;
}) {
  const { t } = useTranslation('alerts');
  const manageAccess = useActionAccess({ permission: 'alerts.rules.manage' });
  const agentAccess = useActionAccess({ permission: 'agent.use' });
  const navigate = useNavigate();

  // The panel opens over the page, and a drawer would cover it: the drawer closes first.
  const investigate = (finding: Finding) => {
    onClose();
    useMoleAgentStore.getState().start({
      prompt: investigatePrompt(finding, t),
      subject: { kind: 'finding', id: finding.id },
    });
  };

  return (
    <FormDrawer
      open={finding !== null}
      onOpenChange={(open) => !open && onClose()}
      title={finding ? findingSummary(finding, t) : ''}
      subtitle={
        finding ? (
          <span className="flex items-center gap-2">
            <Pill tone={SEVERITY_TONE[finding.severity]}>{t(`severity.${finding.severity}`)}</Pill>
            <Pill tone={finding.endedAt ? 'dim' : 'blue'}>
              {finding.endedAt ? t('discovery.tabs.ended') : t('discovery.tabs.open')}
            </Pill>
          </span>
        ) : undefined
      }
      footer={
        finding ? (
          <>
            <ChromeButton onClick={() => navigate(findingHref(finding))}>
              {t(`discovery.open.${sourceOf(finding.kind)}`)}
            </ChromeButton>
            {agentAccess.allowed && (
              <ChromeButton onClick={() => investigate(finding)} data-testid="discovery-investigate">
                <Bot className="h-3.5 w-3.5" />
                {t('discovery.investigate')}
              </ChromeButton>
            )}
            <ChromeButton
              variant="primary"
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
              onClick={() => onRule(finding)}
              data-testid="discovery-rule-from-finding"
            >
              {t('discovery.rule_for_kind')}
            </ChromeButton>
          </>
        ) : null
      }
    >
      {finding && (
        <div className="flex flex-col gap-2" data-finding-detail={finding.id}>
          <FormSection title={t('discovery.sections.what')}>
            <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-2 font-sans text-sm">
              <dt className="text-tx-2">{t('discovery.fields.kind')}</dt>
              <dd className="text-tx-0">{t(`discovery.kinds.${finding.kind}`)}</dd>
              {finding.metric && (
                <>
                  <dt className="text-tx-2">{t('discovery.fields.now')}</dt>
                  <dd className="tabular-nums text-tx-0">{formatFindingValue(finding.metric, finding.value)}</dd>
                  <dt className="text-tx-2">{t('discovery.fields.before')}</dt>
                  <dd className="tabular-nums text-tx-0">
                    {formatFindingValue(finding.metric, finding.detail.previous)}
                  </dd>
                  <dt className="text-tx-2">{t('discovery.fields.last_week')}</dt>
                  <dd className="tabular-nums text-tx-0">
                    {formatFindingValue(finding.metric, finding.detail.lastWeek)}
                  </dd>
                </>
              )}
              {finding.detail.version && (
                <>
                  <dt className="text-tx-2">{t('discovery.fields.version')}</dt>
                  <dd className="text-tx-0">
                    {finding.detail.previousVersion
                      ? t('discovery.version_after', {
                          version: finding.detail.version,
                          previous: finding.detail.previousVersion,
                        })
                      : finding.detail.version}
                  </dd>
                </>
              )}
              {finding.detail.location && (
                <>
                  <dt className="text-tx-2">{t('discovery.fields.location')}</dt>
                  <dd className="break-all font-mono text-xs text-tx-0">{finding.detail.location}</dd>
                </>
              )}
              <dt className="text-tx-2">{t('discovery.fields.started')}</dt>
              <dd className="tabular-nums text-tx-0">{formatMicrosActive(finding.startedAt)}</dd>
              <dt className="text-tx-2">{t('discovery.fields.last_seen')}</dt>
              <dd className="tabular-nums text-tx-0">{formatMicrosActive(finding.lastSeenAt)}</dd>
              {finding.endedAt && (
                <>
                  <dt className="text-tx-2">{t('discovery.fields.ended')}</dt>
                  <dd className="tabular-nums text-tx-0">{formatMicrosActive(finding.endedAt)}</dd>
                </>
              )}
            </dl>
          </FormSection>
          {(finding.detail.changes?.length ?? 0) > 0 && (
            <FormSection title={t('discovery.sections.changes')} className="mb-0">
              <ul className="flex flex-col gap-1.5" data-finding-changes>
                {finding.detail.changes?.map((change) => (
                  <li key={`${change.service}-${change.version}-${change.at}`} className="font-sans text-sm text-tx-1">
                    <span className="tabular-nums text-tx-2">{formatMicrosActive(change.at)}</span>
                    {' · '}
                    {change.service} {change.version}
                    {change.downstream ? ` · ${t('discovery.downstream')}` : ''}
                  </li>
                ))}
              </ul>
            </FormSection>
          )}
        </div>
      )}
    </FormDrawer>
  );
}
