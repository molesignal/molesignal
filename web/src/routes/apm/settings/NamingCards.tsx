import { ArrowRight, Tag, Waypoints } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { EnvironmentMapping, NamingRule } from '@/api/apmSettings';
import type { ActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';

import { mappedTwice, patternReads, renamed } from './form';
import { CardSave, FIELD, Issues, ListRows, type useDraft } from './parts';
import { ApmCard } from '../frame/Page';

type Draft<T> = ReturnType<typeof useDraft<T>>;

/**
 * How service names are rewritten as spans are read: the first rule whose
 * pattern matches a name rewrites it. A name typed in the try field shows
 * what it would be read as.
 */
export function NamingRulesCard({
  draft,
  access,
  pending,
  onSave,
}: {
  draft: Draft<NamingRule[]>;
  access: ActionAccess;
  pending: boolean;
  onSave: () => void;
}) {
  const { t } = useTranslation('apm');
  const [sample, setSample] = React.useState('');
  const rules = draft.value;
  const unread = rules.map((rule) => !patternReads(rule.pattern));
  const result = sample.trim() ? renamed(rules, sample.trim()) : null;

  return (
    <ApmCard
      icon={Tag}
      tone="text-[var(--blue-ink)]"
      title={t('org_settings.naming.title')}
      description={t('org_settings.naming.hint')}
      testId="settings-naming"
      actions={<CardSave name="naming" dirty={draft.dirty} invalid={rules.some((rule) => !rule.pattern.trim())} pending={pending} access={access} onSave={onSave} onReset={draft.reset} />}
    >
      <ListRows
        name="naming"
        rows={rules}
        onChange={draft.set}
        blank={() => ({ pattern: '', replacement: '' })}
        addLabel={t('org_settings.naming.add')}
        empty={t('org_settings.naming.empty')}
        disabled={access.disabled}
        render={(rule, change, index) => (
          <>
            <input
              aria-label={t('org_settings.naming.pattern')}
              placeholder={t('org_settings.naming.pattern_placeholder')}
              aria-invalid={unread[index] && rule.pattern.trim() !== '' ? true : undefined}
              value={rule.pattern}
              disabled={access.disabled}
              onChange={(event) => change({ ...rule, pattern: event.target.value })}
              className={cn(FIELD, 'flex-[3]')}
            />
            <ArrowRight aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
            <input
              aria-label={t('org_settings.naming.replacement')}
              placeholder={t('org_settings.naming.replacement_placeholder')}
              value={rule.replacement}
              disabled={access.disabled}
              onChange={(event) => change({ ...rule, replacement: event.target.value })}
              className={cn(FIELD, 'flex-[2]')}
            />
          </>
        )}
      />
      <div className="flex items-center gap-2 border-t border-bd-0 px-3 py-2 font-sans text-xs">
        <input
          aria-label={t('org_settings.naming.try')}
          placeholder={t('org_settings.naming.try')}
          value={sample}
          onChange={(event) => setSample(event.target.value)}
          className={cn(FIELD, 'w-56')}
        />
        {sample.trim() ? (
          <span data-apm-naming-result="" className="min-w-0 truncate text-tx-2">
            {result ? (
              <>
                {t('org_settings.naming.read_as')} <span className="font-strong text-tx-0">{result}</span>
              </>
            ) : (
              t('org_settings.naming.unchanged')
            )}
          </span>
        ) : (
          <span className="min-w-0 truncate text-tx-3">{t('org_settings.naming.traces_keep')}</span>
        )}
      </div>
      <Issues issues={unread.some((bad, index) => bad && rules[index]?.pattern.trim()) ? [t('org_settings.naming.unread')] : []} />
    </ApmCard>
  );
}

/** Environments named more than one way, read as one: each name to the one it is read as. */
export function EnvironmentMapCard({
  draft,
  access,
  pending,
  onSave,
}: {
  draft: Draft<EnvironmentMapping[]>;
  access: ActionAccess;
  pending: boolean;
  onSave: () => void;
}) {
  const { t } = useTranslation('apm');
  const map = draft.value;
  const twice = mappedTwice(map);
  const incomplete = map.some((mapping) => !mapping.from.trim() || !mapping.to.trim());

  return (
    <ApmCard
      icon={Waypoints}
      tone="text-[var(--chart-2)]"
      title={t('org_settings.environments.title')}
      description={t('org_settings.environments.hint')}
      testId="settings-environments"
      actions={<CardSave name="environments" dirty={draft.dirty} invalid={twice.size > 0 || incomplete} pending={pending} access={access} onSave={onSave} onReset={draft.reset} />}
    >
      <ListRows
        name="environments"
        rows={map}
        onChange={draft.set}
        blank={() => ({ from: '', to: '' })}
        addLabel={t('org_settings.environments.add')}
        empty={t('org_settings.environments.empty')}
        disabled={access.disabled}
        render={(mapping, change, index) => (
          <>
            <input
              aria-label={t('org_settings.environments.from')}
              placeholder={t('org_settings.environments.from')}
              aria-invalid={twice.has(index) || undefined}
              value={mapping.from}
              disabled={access.disabled}
              onChange={(event) => change({ ...mapping, from: event.target.value })}
              className={cn(FIELD, 'flex-1')}
            />
            <ArrowRight aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
            <input
              aria-label={t('org_settings.environments.to')}
              placeholder={t('org_settings.environments.to')}
              value={mapping.to}
              disabled={access.disabled}
              onChange={(event) => change({ ...mapping, to: event.target.value })}
              className={cn(FIELD, 'flex-1')}
            />
          </>
        )}
      />
      <Issues issues={twice.size > 0 ? [t('org_settings.environments.twice')] : []} />
    </ApmCard>
  );
}
