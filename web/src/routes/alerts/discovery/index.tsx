import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as alertsApi from '@/api/alerts';
import { FINDING_KINDS, listFindings, type Finding, type FindingKind } from '@/api/discovery';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { FormSelect } from '@/shell/FormDrawer';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { formatRelativeMicros } from '@/time/relative';
import { SeverityRail } from '@/viz/SeverityRail';

import { AlertsPage } from '../Frame';
import { alertFlatTableClassName } from '../Surfaces';
import { FindingDrawer } from './FindingDrawer';
import { findingSummary } from './model';
import { DiscoveryRuleDrawer } from './RuleDrawer';

type View = 'open' | 'ended';

/**
 * What automatic discovery found without a rule: APM services whose errors, latency or traffic
 * moved, new versions doing worse, new and returning RUM error issues. A finding is only listed;
 * a rule whose source is discovery turns the findings it takes into incidents.
 */
export function AlertsDiscovery() {
  const { t, i18n } = useTranslation('alerts');
  const manageAccess = useActionAccess({ permission: 'alerts.rules.manage' });
  const [view, setView] = React.useState<View>('open');
  const [kind, setKind] = React.useState<FindingKind | ''>('');
  const [viewing, setViewing] = React.useState<Finding | null>(null);
  const [ruleFrom, setRuleFrom] = React.useState<Finding | 'blank' | null>(null);
  // A discovery rule opened from the rules list comes here to be written.
  const [searchParams, setSearchParams] = useSearchParams();
  const editingId = searchParams.get('rule');
  const editing = useQuery({
    queryKey: ['alerts', 'rules', editingId],
    queryFn: () => alertsApi.get(editingId ?? ''),
    enabled: Boolean(editingId),
  });

  const query = useQuery({
    queryKey: ['alerts', 'discovery', view, kind],
    queryFn: () => listFindings({ status: view, ...(kind ? { kind } : {}), limit: 200 }),
    refetchInterval: 60_000,
  });
  const rows = query.data ?? [];
  const nowMicros = Date.now() * 1000;

  const state: ProductStateProps | null = query.isLoading
    ? { variant: 'loading' }
    : query.isError
      ? {
          variant: 'error',
          title: t('discovery.load_error'),
          error: query.error,
          action: <ChromeButton onClick={() => void query.refetch()}>{t('center.retry')}</ChromeButton>,
        }
      : rows.length === 0
        ? {
            variant: 'empty',
            title: t(`discovery.empty.${view}.title`),
            description: t(`discovery.empty.${view}.description`),
          }
        : null;

  const newRule = (
    <ChromeButton
      disabled={manageAccess.disabled}
      disabledReason={manageAccess.reason}
      onClick={() => setRuleFrom('blank')}
      data-testid="discovery-new-rule"
    >
      {t('discovery.new_rule')}
    </ChromeButton>
  );

  return (
    <>
      <AlertsPage
        action={newRule}
        state={state}
        listHead={
          <>
            <ViewTabs<View>
              label={t('discovery.tabs_label')}
              value={view}
              onValueChange={setView}
              dataAttributes={{ 'data-discovery-views': '' }}
              items={[
                { key: 'open', label: t('discovery.tabs.open') },
                { key: 'ended', label: t('discovery.tabs.ended') },
              ]}
            />
            <FormSelect
              ariaLabel={t('discovery.filter_kind')}
              className="ml-auto w-full sm:w-[200px]"
              value={kind}
              onChange={(next) => setKind(next as FindingKind | '')}
              options={[
                { value: '', label: t('discovery.all_kinds') },
                ...FINDING_KINDS.map((item) => ({ value: item, label: t(`discovery.kinds.${item}`) })),
              ]}
            />
          </>
        }
      >
        <DataTable
          className={alertFlatTableClassName}
          rows={rows}
          rowKey={(finding) => finding.id}
          onRowClick={(finding) => setViewing(finding)}
          columns={[
            {
              key: 'finding',
              header: t('discovery.columns.finding'),
              cell: (finding) => (
                <div className="flex min-w-0 items-stretch gap-[12px]" data-finding={finding.kind}>
                  <SeverityRail severity={finding.severity} className="w-[3px] shrink-0 rounded-full" />
                  <div className="min-w-0 py-1">
                    <div className="truncate font-strong text-tx-0">{findingSummary(finding, t)}</div>
                    <div className="mt-0.5 truncate text-xs text-tx-2">
                      {[
                        t(`severity.${finding.severity}`),
                        t(`discovery.kinds.${finding.kind}`),
                        finding.environment,
                      ]
                        .filter(Boolean)
                        .join(' · ')}
                    </div>
                  </div>
                </div>
              ),
            },
            {
              key: 'started',
              header: t('discovery.columns.started'),
              width: 150,
              cell: (finding) => (
                <span className="tabular-nums text-tx-2">
                  {formatRelativeMicros(finding.startedAt, i18n.language, nowMicros)}
                </span>
              ),
            },
            {
              key: 'seen',
              header: view === 'open' ? t('discovery.columns.last_seen') : t('discovery.columns.ended'),
              width: 150,
              cell: (finding) => (
                <span className="tabular-nums text-tx-2">
                  {formatRelativeMicros(finding.endedAt ?? finding.lastSeenAt, i18n.language, nowMicros)}
                </span>
              ),
            },
          ]}
        />
      </AlertsPage>
      <FindingDrawer
        finding={viewing}
        onClose={() => setViewing(null)}
        onRule={(finding) => {
          setViewing(null);
          setRuleFrom(finding);
        }}
      />
      <DiscoveryRuleDrawer
        open={ruleFrom !== null || Boolean(editingId && editing.data)}
        from={ruleFrom === 'blank' ? null : ruleFrom}
        editing={editingId ? (editing.data ?? null) : null}
        onClose={() => {
          setRuleFrom(null);
          if (editingId) setSearchParams({}, { replace: true });
        }}
      />
    </>
  );
}
