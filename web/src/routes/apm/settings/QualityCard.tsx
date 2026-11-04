import { ShieldCheck } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { OverviewResponse } from '@/api/apm';
import type { EnvironmentMapping, NamingRule } from '@/api/apmSettings';
import { useTimeFormatter } from '@/lib/time';

import { caseVariants, generatedBase, renamed, suffixRule } from './form';
import { formatCount } from '../format';
import { ApmCard, ApmQueryState } from '../frame/Page';
import { qualityState } from '../frame/QualityChip';

const ACTION = 'font-strong text-indigo-soft outline-none hover:underline focus-visible:underline';

interface Finding {
  key: string;
  /** A warning, or a suggestion of a setting. */
  tone: 'warning' | 'suggestion';
  text: React.ReactNode;
  action?: React.ReactNode;
}

/**
 * How well APM read the services over the day before: what it could not
 * keep apart past the limits, what it missed or has yet to read, what the
 * services leave out; and the names a naming rule or an environment
 * mapping would read as one, each a setting away.
 */
export function QualityCard({
  query,
  rules,
  mapped,
  limitsShown,
  onAddRule,
  onAddMappings,
}: {
  query: { data?: OverviewResponse | undefined; isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown };
  /** The naming rules as they stand, saved or not: a name they rename is not suggested again. */
  rules: readonly NamingRule[];
  /** The environments mapped, saved or not. */
  mapped: readonly EnvironmentMapping[];
  limitsShown: boolean;
  onAddRule: (rule: NamingRule) => void;
  onAddMappings: (mappings: EnvironmentMapping[]) => void;
}) {
  const { t } = useTranslation('apm');
  const time = useTimeFormatter();
  const data = query.data;
  const findings: Finding[] = [];

  if (data) {
    const { meta, instrumentation } = data;
    const overflow = meta.dataQuality.overflowDimensions;

    if (overflow.length > 0) {
      findings.push({
        key: 'overflow',
        tone: 'warning',
        text: t('org_settings.quality.overflow', { kinds: overflow.map((kind) => t(`org_settings.quality.kinds.${kind}`, { defaultValue: kind })).join(t('org_settings.quality.and')) }),
        action: limitsShown ? (
          <button type="button" className={ACTION} onClick={() => document.querySelector('[data-apm-card="settings-limits"]')?.scrollIntoView({ block: 'start', behavior: 'smooth' })}>
            {t('org_settings.quality.raise_limits')}
          </button>
        ) : (
          <span className="text-tx-3">{t('org_settings.quality.limits_by_platform')}</span>
        ),
      });
    }

    if (meta.dataQuality.gaps.length > 0) {
      findings.push({ key: 'gaps', tone: 'warning', text: t('org_settings.quality.gaps', { count: meta.dataQuality.gaps.length }) });
    }

    if (qualityState(meta) === 'stale' && meta.lastCompleteBucketAt) {
      findings.push({ key: 'stale', tone: 'warning', text: t('org_settings.quality.stale', { at: time.micros(meta.lastCompleteBucketAt, false) }) });
    }

    const attributes = [
      ['without_environment', instrumentation?.withoutEnvironment ?? 0, 'deployment.environment.name'],
      ['without_version', instrumentation?.withoutVersion ?? 0, 'service.version'],
      ['without_instance', instrumentation?.withoutInstance ?? 0, 'service.instance.id'],
    ] as const;

    for (const [key, count, attribute] of attributes) {
      if (count === 0) continue;
      findings.push({
        key,
        tone: 'warning',
        text: (
          <>
            {t('org_settings.quality.missing', { count, formatted: formatCount(count) })} <code className="font-sans text-tx-1">{attribute}</code>
          </>
        ),
        action: (
          <Link to="/datasource/applications/opentelemetry?signal=traces" className={ACTION}>
            {t('org_settings.quality.instrument')}
          </Link>
        ),
      });
    }

    const mappedFrom = new Set(mapped.map((mapping) => mapping.from.trim().toLocaleLowerCase()));

    for (const names of caseVariants(data.availableEnvironments ?? [])) {
      const target = names.find((name) => name === name.toLocaleLowerCase()) ?? names[0] ?? '';
      const others = names.filter((name) => name !== target && !mappedFrom.has(name.toLocaleLowerCase()));

      if (others.length === 0) continue;
      findings.push({
        key: `environment:${target}`,
        tone: 'suggestion',
        text: t('org_settings.quality.environments', { names: names.join(t('org_settings.quality.and')) }),
        action: (
          <button type="button" className={ACTION} onClick={() => onAddMappings(others.map((from) => ({ from, to: target })))}>
            {t('org_settings.quality.map_to', { name: target })}
          </button>
        ),
      });
    }

    const suggested = new Set<string>();

    for (const summary of data.services) {
      const name = summary.service.name;
      const base = generatedBase(name);
      const rule = suffixRule(name);

      if (!base || !rule || suggested.has(base) || renamed(rules, name) !== null) continue;
      suggested.add(base);
      findings.push({
        key: `naming:${base}`,
        tone: 'suggestion',
        text: t('org_settings.quality.generated', { name, base }),
        action: (
          <button type="button" className={ACTION} onClick={() => onAddRule(rule)}>
            {t('org_settings.quality.add_rule')}
          </button>
        ),
      });
    }
  }

  return (
    <ApmCard icon={ShieldCheck} tone="text-[var(--blue-ink)]" title={t('org_settings.quality.title')} description={t('org_settings.quality.hint')} testId="settings-quality">
      <ApmQueryState query={query} empty={false} compact>
        {findings.length === 0 ? (
          <p className="px-3 py-3 font-sans text-xs font-strong text-[var(--green-ink)]">{t('org_settings.quality.clean')}</p>
        ) : (
          <ul className="divide-y divide-bd-0">
            {findings.map((finding) => (
              <li key={finding.key} data-apm-quality-finding={finding.key} className="flex min-h-8 flex-wrap items-center gap-x-3 gap-y-1 px-3 py-1.5 font-sans text-xs">
                <span className={finding.tone === 'warning' ? 'shrink-0 font-strong text-[var(--yellow-ink)]' : 'shrink-0 font-strong text-[var(--blue-ink)]'}>
                  {t(`org_settings.quality.tone.${finding.tone}`)}
                </span>
                <span className="min-w-0 flex-1 text-tx-1">{finding.text}</span>
                {finding.action}
              </li>
            ))}
          </ul>
        )}
      </ApmQueryState>
    </ApmCard>
  );
}
