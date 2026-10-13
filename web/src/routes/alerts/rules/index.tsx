import { useQuery } from '@tanstack/react-query';
import { Pencil, Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as alertsApi from '@/api/alerts';
import * as incidentsApi from '@/api/incidents';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ChromeButton, Dot, IconButton, Pill, type PillTone } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';
import { ViewTabs } from '@/shell/tabs/ViewTabs';

import {
  type DisplayRule,
  type RuleDisplayState,
  adaptRule,
  formatDurationSecs,
} from '../alertRuleModel';
import { AlertsPage } from '../Frame';
import { AlertsActivation } from '../incidents/Activation';
import { alertFlatTableClassName } from '../Surfaces';
import { NewRuleMenu } from './NewRule';

type RuleView = 'all' | 'enabled' | 'disabled';

const RULE_STATE_TONE: Record<RuleDisplayState, PillTone> = {
  not_evaluated: 'blue',
  healthy: 'green',
  pending: 'yellow',
  firing: 'red',
  disabled: 'dim',
};

/**
 * How long ago something happened, in the units a rule is evaluated in: a rule
 * runs every few seconds, and "now" on every row would say nothing.
 */
function compactAge(micros: number, nowMicros: number): string {
  return formatDurationSecs(Math.max(0, Math.round((nowMicros - micros) / 1_000_000)));
}

function rulePath(rule: DisplayRule): string {
  return rule.kind === 'anomaly' ? `/alerts/anomaly/edit/${rule.id}` : `/alerts/rules/${rule.id}/edit`;
}

/**
 * Every rule the system evaluates: what it watches, when it fires, and how it
 * stands now. The search and the enabled/disabled views are the head of the
 * list; they carry the counts, so the list states no summary of its own.
 */
export function AlertRulesPage() {
  const { t } = useTranslation('alerts');
  const navigate = useNavigate();
  const manageAccess = useActionAccess({ permission: 'alerts.manage' });
  const [view, setView] = React.useState<RuleView>('all');
  const [search, setSearch] = React.useState('');

  const rulesQuery = useQuery({
    queryKey: ['alerts', 'rules'],
    queryFn: () => alertsApi.list(),
    refetchInterval: 30_000,
  });
  const incidentsQuery = useQuery({
    queryKey: ['alerts', 'incidents', 'active'],
    queryFn: () => incidentsApi.list(),
    refetchInterval: 30_000,
  });
  const rules = React.useMemo(
    () => (rulesQuery.data ?? []).map((rule) => adaptRule(rule, incidentsQuery.data ?? [])),
    [incidentsQuery.data, rulesQuery.data],
  );
  const enabled = rules.filter((rule) => rule.raw.enabled).length;
  const needle = search.trim().toLowerCase();
  const filtered = rules.filter((rule) => {
    if (view === 'enabled' && !rule.raw.enabled) return false;
    if (view === 'disabled' && rule.raw.enabled) return false;

    return (
      !needle
      || rule.name.toLowerCase().includes(needle)
      || rule.service.toLowerCase().includes(needle)
      || rule.source.toLowerCase().includes(needle)
    );
  });

  const nowMicros = Date.now() * 1000;
  const loading = rulesQuery.isLoading || incidentsQuery.isLoading;
  const failed = rulesQuery.isError || incidentsQuery.isError;

  if (!loading && !failed && rules.length === 0) {
    // No rule yet: the first run carries the one way forward, so the header offers nothing.
    return (
      <AlertsPage contentSurface={false}>
        <AlertsActivation />
      </AlertsPage>
    );
  }

  const state: ProductStateProps | null = loading
    ? { variant: 'loading' }
    : failed
      ? {
          variant: 'error',
          title: t('center.rules.load_error'),
          error: rulesQuery.error ?? incidentsQuery.error,
          action: (
            <ChromeButton
              onClick={() => {
                void rulesQuery.refetch();
                void incidentsQuery.refetch();
              }}
            >
              {t('center.retry')}
            </ChromeButton>
          ),
        }
      : filtered.length === 0
        ? {
            variant: 'empty',
            title: t('center.rules.no_match.title'),
            description: t('center.rules.no_match.description'),
          }
        : null;

  return (
    <AlertsPage
      action={<NewRuleMenu />}
      state={state}
      listHead={
        <>
          <label className="relative w-full min-w-0 sm:w-[300px]">
            <span className="sr-only">{t('center.rules.search')}</span>
            <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-tx-3" />
            <FormInput
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t('center.rules.search')}
              className="pl-9"
            />
          </label>
          <ViewTabs<RuleView>
            label={t('center.rules.tabs_label')}
            value={view}
            onValueChange={setView}
            dataAttributes={{ 'data-alert-filter-tabs': '' }}
            items={[
              { key: 'all', label: t('center.rules.tabs.all'), count: rules.length },
              { key: 'enabled', label: t('center.rules.tabs.enabled'), count: enabled },
              { key: 'disabled', label: t('center.rules.tabs.disabled'), count: rules.length - enabled },
            ]}
          />
        </>
      }
    >
      <DataTable
        className={alertFlatTableClassName}
        rows={filtered}
        rowKey={(rule) => rule.id}
        onRowClick={(rule) => navigate(rulePath(rule))}
        isRowClickDisabled={() => manageAccess.disabled}
        rowClickDisabledReason={() => manageAccess.reason}
        columns={[
          {
            key: 'rule',
            header: t('center.rules.columns.rule'),
            cell: (rule) => (
              <div className="min-w-0 py-1">
                <div className="truncate font-strong text-tx-0">{rule.name}</div>
                <div className="mt-0.5 truncate text-xs text-tx-2">
                  {t(`center.rules.kinds.${rule.kind}`)}
                  {rule.service !== '—' ? ` · ${rule.service}` : ''}
                </div>
              </div>
            ),
          },
          {
            key: 'source',
            header: t('center.rules.columns.source'),
            width: 250,
            cell: (rule) => <span className="block truncate text-tx-1">{rule.source}</span>,
          },
          {
            key: 'condition',
            header: t('center.rules.columns.condition'),
            width: 180,
            // A condition is read, not followed: it is set as data, in the page's ink.
            cell: (rule) => <span className="font-mono text-xs text-tx-1">{rule.condition}</span>,
          },
          {
            key: 'state',
            header: t('center.rules.columns.state'),
            width: 150,
            cell: (rule) => (
              <Pill tone={RULE_STATE_TONE[rule.state]}>
                {rule.state === 'firing' && <Dot tone="red" />}
                {t(`center.rules.state.${rule.state}`)}
              </Pill>
            ),
          },
          {
            key: 'lastEvaluation',
            header: t('center.rules.columns.last_evaluation'),
            width: 160,
            cell: (rule) => (
              <span className="text-tx-2">
                {rule.lastEvaluation
                  ? t('center.rules.evaluated_ago', { age: compactAge(rule.lastEvaluation, nowMicros) })
                  : t('center.rules.never_evaluated')}
              </span>
            ),
          },
          {
            key: 'actions',
            header: t('center.rules.columns.actions'),
            width: 84,
            className: 'overflow-visible text-right',
            headerClassName: 'text-right',
            cell: (rule) => (
              <IconButton
                disabled={manageAccess.disabled}
                disabledReason={manageAccess.reason}
                onClick={(event) => {
                  event.stopPropagation();
                  navigate(rulePath(rule));
                }}
                className="ml-auto"
                aria-label={t('actions.edit_rule', { name: rule.name })}
              >
                <Pencil className="h-4 w-4" />
              </IconButton>
            ),
          },
        ]}
      />
    </AlertsPage>
  );
}
