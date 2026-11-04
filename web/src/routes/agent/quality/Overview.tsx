import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as qualityApi from '@/api/agent/quality';
import type { GroupQuality, OverviewResponse } from '@/api/agent/quality';
import { ProductState } from '@/product/states';
import { uiSectionTitleClass } from '@/shell/chrome';
import { Facts, type Fact } from '@/shell/readout/Facts';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shell/ui/table';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { QualityFilters } from './Filters';
import { formatScore, trendPoints } from './model';
import { ScoreWord } from './ScoreWord';
import { QUALITY_KEY, useQualityFilter, useScoringSettings } from './state';
import { EmptyTab, ModulePage } from '../operations/Pages';

/**
 * How good Mole Agent's answers were over a period: the totals, the daily
 * trend of the rules and of the evaluators, each rule dimension, and the answers
 * split by where they came from, the prompt version, the Skill and the model.
 */
export function QualityOverview() {
  const { t } = useTranslation('agent');
  const navigate = useNavigate();
  const filter = useQualityFilter();
  const { threshold } = useScoringSettings();
  const overview = useQuery({
    queryKey: [...QUALITY_KEY, 'overview', filter.days, filter.source ?? 'all'],
    queryFn: () => qualityApi.overview({ days: filter.days, source: filter.source }),
    retry: false,
  });

  const filters = (
    <QualityFilters
      days={filter.days}
      source={filter.source}
      onDays={filter.setDays}
      onSource={filter.setSource}
    />
  );

  return (
    <ModulePage title={t('quality.title')} description={t('quality.description')} action={filters}>
      {overview.isPending ? (
        <ProductState variant="loading" />
      ) : overview.isError ? (
        <ProductState variant="error" error={overview.error} />
      ) : overview.data.overview.answers === 0 ? (
        <EmptyTab title={t('quality.empty_title')} hint={t('quality.empty_hint')} />
      ) : (
        <div data-quality-overview className="flex flex-col gap-6">
          <Totals
            data={overview.data}
            threshold={threshold}
            onReviews={() => navigate('/agent/quality/reviews')}
          />
          <Trend data={overview.data} />
          <Dimensions data={overview.data} threshold={threshold} />
          <div className="grid gap-6 xl:grid-cols-2">
            <Groups
              title={t('quality.groups.sources')}
              rows={overview.data.overview.sources}
              threshold={threshold}
              name={(row) => t(`quality.sources.${row.key}`, { defaultValue: row.key })}
            />
            <Groups
              title={t('quality.groups.prompts')}
              rows={overview.data.overview.prompts}
              threshold={threshold}
              name={(row) => promptName(t, overview.data, row)}
            />
            <Groups
              title={t('quality.groups.skills')}
              rows={overview.data.overview.skills}
              threshold={threshold}
              name={(row) => (row.version ? `/${row.key} · ${row.version}` : `/${row.key}`)}
            />
            <Groups
              title={t('quality.groups.models')}
              rows={overview.data.overview.models}
              threshold={threshold}
              name={(row) => row.key}
            />
          </div>
        </div>
      )}
    </ModulePage>
  );
}

function promptName(t: TFunction, data: OverviewResponse, row: GroupQuality): string {
  const name = data.promptNames[row.key] ?? row.key;
  return row.version ? t('quality.groups.prompt_version', { name, version: row.version }) : name;
}

function Totals({
  data,
  threshold,
  onReviews,
}: {
  data: OverviewResponse;
  threshold: number;
  onReviews: () => void;
}) {
  const { t } = useTranslation('agent');
  const overview = data.overview;
  const tone = (mean: number | null): Fact['tone'] =>
    mean === null ? undefined : mean < threshold ? 'danger' : undefined;

  const facts: Fact[] = [
    { key: 'answers', label: t('quality.totals.answers'), value: overview.answers.toLocaleString() },
    {
      key: 'rules',
      label: t('quality.totals.rules_mean'),
      value: formatScore(overview.rulesMean),
      tone: tone(overview.rulesMean),
      hint: t('quality.totals.rules_hint'),
    },
    { key: 'evaluated', label: t('quality.totals.evaluated'), value: overview.evaluated.toLocaleString() },
    {
      key: 'evaluators',
      label: t('quality.totals.evaluator_mean'),
      value: formatScore(overview.evaluatorMean),
      tone: tone(overview.evaluatorMean),
      hint: t('quality.totals.evaluator_hint'),
    },
    { key: 'up', label: t('quality.totals.thumbs_up'), value: overview.thumbsUp.toLocaleString() },
    {
      key: 'down',
      label: t('quality.totals.thumbs_down'),
      value: overview.thumbsDown.toLocaleString(),
      tone: overview.thumbsDown > 0 ? 'warning' : undefined,
    },
    {
      key: 'reviews',
      label: t('quality.totals.pending_reviews'),
      value: overview.pendingReviews.toLocaleString(),
      tone: overview.pendingReviews > 0 ? 'warning' : undefined,
      onPress: onReviews,
    },
  ];

  return <Facts label={t('quality.totals.label')} facts={facts} data-quality-totals="" />;
}

function Trend({ data }: { data: OverviewResponse }) {
  const { t } = useTranslation('agent');
  const points = React.useMemo(() => trendPoints(data.overview.daily), [data.overview.daily]);
  const end = Date.now();
  const start = end - data.days * 86_400_000;

  return (
    <section aria-labelledby="quality-trend">
      <h3 id="quality-trend" className={`m-0 ${uiSectionTitleClass}`}>
        {t('quality.trend.title')}
      </h3>
      <div className="mt-2 h-[180px]">
        <TimeSeriesChart
          series={[
            {
              id: 'rules',
              name: t('quality.trend.rules'),
              color: 'var(--chart-1)',
              data: points.rules,
              timestamps: points.timestamps,
            },
            {
              id: 'evaluators',
              name: t('quality.trend.evaluators'),
              color: 'var(--chart-2)',
              data: points.evaluators,
              timestamps: points.timestamps,
            },
          ]}
          xDomain={[start, end]}
          height="100%"
          ariaLabel={t('quality.trend.title')}
          legendDensity="compact"
          options={{
            showPoints: 'always',
            compactAxes: true,
            legendMode: 'list',
            legendStats: [],
            leftAxis: { min: 0, max: 100 },
          }}
        />
      </div>
    </section>
  );
}

function Dimensions({ data, threshold }: { data: OverviewResponse; threshold: number }) {
  const { t } = useTranslation('agent');
  const rows = qualityApi.RULE_DIMENSIONS.flatMap((dimension) => {
    const found = data.overview.dimensions.find((row) => row.dimension === dimension);
    return found ? [found] : [];
  });

  return (
    <section aria-labelledby="quality-dimensions">
      <h3 id="quality-dimensions" className={`m-0 ${uiSectionTitleClass}`}>
        {t('quality.dimensions.title')}
      </h3>
      <Table className="mt-1 text-xs" data-quality-dimensions="">
        <TableHeader>
          <TableRow>
            <TableHead>{t('quality.dimensions.column')}</TableHead>
            <TableHead className="text-right">{t('quality.columns.score')}</TableHead>
            <TableHead className="text-right">{t('quality.columns.answers')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => (
            <TableRow key={row.dimension} data-quality-dimension={row.dimension}>
              <TableCell>
                <span className="text-tx-1">{t(`quality.dimensions.${row.dimension}.title`)}</span>
                <span className="ml-2 text-tx-3">{t(`quality.dimensions.${row.dimension}.hint`)}</span>
              </TableCell>
              <TableCell className="text-right">
                <ScoreWord score={row.mean} threshold={threshold} />
              </TableCell>
              <TableCell className="text-right">{row.answers.toLocaleString()}</TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </section>
  );
}

function Groups({
  title,
  rows,
  threshold,
  name,
}: {
  title: string;
  rows: readonly GroupQuality[];
  threshold: number;
  name: (row: GroupQuality) => string;
}) {
  const { t } = useTranslation('agent');

  return (
    <section aria-label={title} className="min-w-0">
      <h3 className={`m-0 ${uiSectionTitleClass}`}>{title}</h3>
      {rows.length === 0 ? (
        <p className="m-0 mt-2 font-sans text-xs text-tx-3">{t('quality.groups.none')}</p>
      ) : (
        <Table className="mt-1 text-xs">
          <TableHeader>
            <TableRow>
              <TableHead>{t('quality.columns.name')}</TableHead>
              <TableHead className="text-right">{t('quality.columns.answers')}</TableHead>
              <TableHead className="text-right">{t('quality.columns.rules')}</TableHead>
              <TableHead className="text-right">{t('quality.columns.evaluated')}</TableHead>
              <TableHead className="text-right">{t('quality.columns.evaluators')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <TableRow key={`${row.key}:${row.version ?? ''}`}>
                <TableCell className="max-w-0 truncate" title={name(row)}>
                  {name(row)}
                </TableCell>
                <TableCell className="text-right">{row.answers.toLocaleString()}</TableCell>
                <TableCell className="text-right">
                  <ScoreWord score={row.rulesMean} threshold={threshold} />
                </TableCell>
                <TableCell className="text-right">{row.evaluated.toLocaleString()}</TableCell>
                <TableCell className="text-right">
                  <ScoreWord score={row.evaluated > 0 ? row.evaluatorMean : null} threshold={threshold} />
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </section>
  );
}
