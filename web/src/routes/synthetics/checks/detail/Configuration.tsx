import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { MonitorRevision, ProbeLocation } from '@/api/synthetics';

import { SyntheticsSection } from '../../Frame';
import { monitorTarget, scheduleText } from '../../model';

/**
 * What the published revision of a check is set to do: what it calls and how
 * often, when it counts as failed, and who hears about it. The definition the
 * probe runs is under it, as it is stored.
 */
export function CheckConfiguration({
  revision,
  locations,
}: {
  revision: MonitorRevision;
  locations: readonly ProbeLocation[];
}) {
  const { t } = useTranslation('synthetics');
  const heartbeat = revision.spec.kind === 'heartbeat';
  const policy = revision.location_policy;
  const yesNo = (value: boolean) => t(value ? 'common.yes' : 'common.no');
  const facts: Array<[label: string, value: React.ReactNode]> = [
    [t('checks.columns.type'), t(`kinds.${revision.spec.kind}`)],
    ...(heartbeat
      ? []
      : ([[t('checks.columns.target'), <code key="target" className="break-all font-code text-xs">{monitorTarget(revision.spec)}</code>]] as Array<[string, React.ReactNode]>)),
    [t(heartbeat ? 'detail.schedule_heartbeat' : 'checks.columns.interval'), scheduleText(revision.schedule)],
    // The label names the unit.
    [t('editor.timeout'), revision.timeout_millis.toLocaleString()],
    [t('editor.retries'), String(revision.max_retries)],
    [t('editor.failure_threshold'), String(revision.consecutive_failures)],
    [t('editor.recovery_threshold'), String(revision.consecutive_recoveries)],
    ...(heartbeat
      ? []
      : ([
          [
            t('editor.locations'),
            revision.location_ids
              .map((id) => locations.find((location) => location.id === id)?.name ?? id)
              .join(', ') || '—',
          ],
          [
            t('detail.locations_policy'),
            policy.kind === 'quorum'
              ? t('detail.policy.quorum', { required: policy.required })
              : t(`detail.policy.${policy.kind}`),
          ],
        ] as Array<[string, React.ReactNode]>)),
    [
      t('detail.notifies'),
      t(revision.escalation_policy_id ? 'detail.notifies_policy' : 'detail.notifies_nobody'),
    ],
    [t('editor.alert_on_degraded'), yesNo(revision.alert_on_degraded)],
    [t('editor.alert_on_flaky'), yesNo(revision.alert_on_flaky)],
  ];

  return (
    <div className="grid grid-cols-1 items-start gap-[12px] xl:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
      <SyntheticsSection title={t('detail.configuration')}>
        <dl className="divide-y divide-bd-0 border-t border-bd-0">
          {facts.map(([label, value]) => (
            <div
              key={label}
              className="grid grid-cols-[minmax(120px,0.8fr)_minmax(0,1.4fr)] gap-[16px] px-4 py-[10px] font-sans text-sm"
            >
              <dt className="text-tx-2">{label}</dt>
              <dd className="min-w-0 text-tx-0">{value}</dd>
            </div>
          ))}
        </dl>
      </SyntheticsSection>
      <SyntheticsSection title={t('detail.request')}>
        {/* It scrolls, so the keyboard has to be able to reach it. */}
        <pre
          tabIndex={0}
          aria-label={t('detail.request')}
          className="max-h-[520px] overflow-auto border-t border-bd-0 p-4 font-code text-xs leading-relaxed text-tx-1 outline-none focus-visible:bg-bg-2"
        >
          {JSON.stringify(
            'configuration' in revision.spec ? revision.spec.configuration : revision.spec,
            null,
            2,
          )}
        </pre>
      </SyntheticsSection>
    </div>
  );
}
