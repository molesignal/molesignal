import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as usageApi from '@/api/agent/usage';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Facts, type Fact } from '@/shell/readout/Facts';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shell/ui/table';

import {
  budgetInput,
  formatTokens,
  limitChoice,
  statusDayStart,
  usageByDay,
  usedPercent,
  type LimitChoice,
} from './model';
import { formatCosts } from '../money';
import { AgentSettingsSection } from '../Surfaces';
import { useCurrencies } from '../useCurrencies';

const USAGE_KEY = ['agent', 'usage'] as const;

/** Settings → Usage: today's tokens against the day's budget, the budget, and the last 30 days. */
export function UsagePanel() {
  const { t } = useTranslation('agent');
  const usage = useQuery({
    queryKey: USAGE_KEY,
    queryFn: () => usageApi.getUsage(30),
    retry: false,
  });

  if (usage.isLoading) return <ProductState variant="loading" />;
  if (usage.isError) return <ProductState variant="error" error={usage.error} />;
  if (!usage.data) return null;

  const days = usageByDay(usage.data.days);
  const today = days.find(
    (day) => day.dayStartMicros === statusDayStart(usage.data.status),
  );

  return (
    <AgentSettingsSection
      title={t('settings.usage.title')}
      description={t('settings.usage.description')}
    >
      <div className="flex flex-col gap-5" data-agent-usage>
        <TodayUsage
          status={usage.data.status}
          today={today}
          costKnown={usage.data.cost_known}
        />
        <BudgetForm budget={usage.data.budget} />
        <UsageHistory days={days} costKnown={usage.data.cost_known} />
      </div>
    </AgentSettingsSection>
  );
}

const LEVEL_FILL: Record<usageApi.BudgetLevel, string> = {
  unlimited: 'bg-indigo',
  ok: 'bg-indigo',
  warning: 'bg-yellow',
  exhausted: 'bg-red',
};

function TodayUsage({
  status,
  today,
  costKnown,
}: {
  status: usageApi.BudgetStatus;
  today: ReturnType<typeof usageByDay>[number] | undefined;
  costKnown: boolean;
}) {
  const { t } = useTranslation('agent');
  const { symbolOf } = useCurrencies();
  const percent = usedPercent(status);

  const facts: Fact[] = [
    {
      key: 'used',
      label: t('settings.usage.today'),
      value: formatTokens(status.used_tokens),
      tone:
        status.level === 'exhausted'
          ? 'danger'
          : status.level === 'warning'
            ? 'warning'
            : undefined,
    },
    {
      key: 'limit',
      label: t('settings.usage.limit'),
      value:
        status.limit_tokens > 0
          ? formatTokens(status.limit_tokens)
          : t('settings.usage.unlimited'),
    },
    ...(percent === null
      ? []
      : [{ key: 'percent', label: t('settings.usage.used_share'), value: `${percent}%` }]),
    {
      key: 'resets',
      label: t('settings.usage.resets'),
      value: formatMicrosActive(status.resets_at, false),
    },
    ...(costKnown && today
      ? [
          {
            key: 'cost',
            label: t('settings.usage.cost_today'),
            value: formatCosts(today.costs, today.unpricedCalls > 0, symbolOf),
          },
        ]
      : []),
  ];

  const origins: Fact[] = usageApi.USAGE_ORIGINS.map((origin) => ({
    key: origin,
    label: t(`settings.usage.origins.${origin}`),
    value: formatTokens(today?.byOrigin[origin].tokens ?? 0),
    hint: t('settings.usage.calls_hint', { calls: today?.byOrigin[origin].calls ?? 0 }),
  }));

  return (
    <div className="flex flex-col gap-2.5" data-agent-usage-today={status.level}>
      <Facts label={t('settings.usage.today')} facts={facts} />
      {percent !== null && (
        <div
          role="meter"
          aria-label={t('settings.usage.used_share')}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.min(100, percent)}
          className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--control-surface)]"
        >
          <div
            className={cn('h-full rounded-full', LEVEL_FILL[status.level])}
            style={{ width: `${Math.min(100, percent)}%` }}
          />
        </div>
      )}
      <Facts label={t('settings.usage.by_origin')} facts={origins} />
    </div>
  );
}

function BudgetForm({ budget }: { budget: usageApi.BudgetSettings }) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const [choice, setChoice] = React.useState<LimitChoice>(() => limitChoice(budget.org_limit));
  const [custom, setCustom] = React.useState(() =>
    budget.org_limit && budget.org_limit > 0 ? String(budget.org_limit) : '',
  );
  const [mode, setMode] = React.useState<usageApi.InteractiveBudgetMode>(budget.interactive_mode);

  React.useEffect(() => {
    setChoice(limitChoice(budget.org_limit));
    setCustom(budget.org_limit && budget.org_limit > 0 ? String(budget.org_limit) : '');
    setMode(budget.interactive_mode);
  }, [budget.org_limit, budget.interactive_mode]);

  const save = useMutation({
    mutationFn: usageApi.putBudget,
    onSuccess: (data) => {
      queryClient.setQueryData(USAGE_KEY, data);
      toast.success(t('settings.usage.saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const instanceDefault =
    budget.instance_default > 0
      ? t('settings.usage.tokens_value', { value: formatTokens(budget.instance_default) })
      : t('settings.usage.unlimited');

  return (
    <form
      data-agent-usage-budget
      className="grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] md:items-end"
      onSubmit={(event) => {
        event.preventDefault();
        const input = budgetInput(choice, custom, mode);
        if (!input) {
          toast.error(t('settings.usage.invalid_limit'));
          return;
        }
        save.mutate(input);
      }}
    >
      <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,9rem)] sm:items-end">
        <FormField label={t('settings.usage.daily_limit')}>
          <FormSelect
            value={choice}
            onChange={(value) => setChoice(value as LimitChoice)}
            ariaLabel={t('settings.usage.daily_limit')}
            options={[
              {
                value: 'inherit',
                label: t('settings.usage.limit_choice.inherit', { value: instanceDefault }),
              },
              { value: 'custom', label: t('settings.usage.limit_choice.custom') },
              { value: 'unlimited', label: t('settings.usage.limit_choice.unlimited') },
            ]}
          />
        </FormField>
        {choice === 'custom' && (
          <FormField label={t('settings.usage.custom_limit')}>
            <FormInput
              type="number"
              min={1}
              inputMode="numeric"
              value={custom}
              onChange={(event) => setCustom(event.target.value)}
              placeholder="200000"
            />
          </FormField>
        )}
      </div>
      <FormField label={t('settings.usage.interactive_mode')}>
        <FormSelect
          value={mode}
          onChange={(value) => setMode(value as usageApi.InteractiveBudgetMode)}
          ariaLabel={t('settings.usage.interactive_mode')}
          options={[
            { value: 'block', label: t('settings.usage.interactive.block') },
            { value: 'warn', label: t('settings.usage.interactive.warn') },
          ]}
        />
      </FormField>
      <Button type="submit" disabled={save.isPending}>
        {t('settings.usage.save')}
      </Button>
    </form>
  );
}

function UsageHistory({
  days,
  costKnown,
}: {
  days: ReturnType<typeof usageByDay>;
  costKnown: boolean;
}) {
  const { t } = useTranslation('agent');
  const { symbolOf } = useCurrencies();
  const unpricedCalls = days.reduce((sum, day) => sum + day.unpricedCalls, 0);

  return (
    <section aria-label={t('settings.usage.history')} data-agent-usage-history>
      <h3 className="pb-2 font-sans text-xs font-strong text-tx-1">{t('settings.usage.history')}</h3>
      {days.length === 0 ? (
        <p className="font-sans text-xs text-tx-3">{t('settings.usage.empty')}</p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t('settings.usage.columns.day')}</TableHead>
              {usageApi.USAGE_ORIGINS.map((origin) => (
                <TableHead key={origin} className="text-right">
                  {t(`settings.usage.origins.${origin}`)}
                </TableHead>
              ))}
              <TableHead className="text-right">{t('settings.usage.columns.total')}</TableHead>
              <TableHead className="text-right">{t('settings.usage.columns.calls')}</TableHead>
              {costKnown && (
                <TableHead className="text-right">{t('settings.usage.columns.cost')}</TableHead>
              )}
            </TableRow>
          </TableHeader>
          <TableBody>
            {days.map((day) => (
              <TableRow key={day.dayStartMicros}>
                <TableCell>{utcDay(day.dayStartMicros)}</TableCell>
                {usageApi.USAGE_ORIGINS.map((origin) => (
                  <TableCell key={origin} className="text-right">
                    {formatTokens(day.byOrigin[origin].tokens)}
                  </TableCell>
                ))}
                <TableCell className="text-right font-strong text-tx-0">
                  {formatTokens(day.tokens)}
                </TableCell>
                <TableCell className="text-right">{formatTokens(day.calls)}</TableCell>
                {costKnown && (
                  <TableCell className="text-right">
                    {formatCosts(day.costs, day.unpricedCalls > 0, symbolOf)}
                  </TableCell>
                )}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
      {!costKnown && (
        <p className="pt-2 font-sans text-xs text-tx-3">{t('settings.usage.cost_unknown')}</p>
      )}
      {costKnown && unpricedCalls > 0 && (
        <p className="pt-2 font-sans text-xs text-tx-3" data-agent-usage-unpriced>
          {t('settings.usage.cost_partial', { calls: formatTokens(unpricedCalls) })}
        </p>
      )}
    </section>
  );
}

/** Usage is counted per UTC day, so the day is named in UTC too. */
function utcDay(dayStartMicros: number): string {
  return new Date(dayStartMicros / 1000).toISOString().slice(0, 10);
}
