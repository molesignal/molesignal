import { ArrowRight, ClipboardCheck, Combine } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ServiceTier } from '@/api/apmCatalog';
import type { MetadataRules, ServiceAlias } from '@/api/apmSettings';
import type { ActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { aliasIssues } from './form';
import { CardSave, FIELD, Issues, ListRows, type useDraft } from './parts';
import { ApmCard } from '../frame/Page';

type Draft<T> = ReturnType<typeof useDraft<T>>;

const TIERS: readonly ServiceTier[] = ['p0', 'p1', 'p2', 'p3', 'p4', 'p5'];
const NONE = 'none';
const NAMES_LIST = 'apm-settings-service-names';

/**
 * Services known under an older name: what an alias reported is read as
 * its service's, on every APM page, merged with what the service reported
 * itself. Taking an alias away reads the two apart again.
 */
export function AliasesCard({
  draft,
  services,
  access,
  pending,
  onSave,
}: {
  draft: Draft<ServiceAlias[]>;
  /** Names of the services seen, offered as the fields are typed. */
  services: readonly string[];
  access: ActionAccess;
  pending: boolean;
  onSave: () => void;
}) {
  const { t } = useTranslation('apm');
  const aliases = draft.value;
  const issues = aliasIssues(aliases);
  // A row just added says nothing until something is typed in it.
  const said = [
    ...new Set(
      [...issues]
        .filter(([index, issue]) => issue !== 'incomplete' || [aliases[index]?.alias, aliases[index]?.service].some((name) => name?.trim()))
        .map(([, issue]) => issue),
    ),
  ].map((issue) => t(`org_settings.aliases.issues.${issue}`));
  const field = (row: number, wide: boolean) => cn(FIELD, wide ? 'flex-[3]' : 'flex-[2]', issues.has(row) && 'text-[var(--red-ink)]');

  return (
    <ApmCard
      icon={Combine}
      tone="text-[var(--purple-ink)]"
      title={t('org_settings.aliases.title')}
      description={t('org_settings.aliases.hint')}
      testId="settings-aliases"
      actions={<CardSave name="aliases" dirty={draft.dirty} invalid={issues.size > 0} pending={pending} access={access} onSave={onSave} onReset={draft.reset} />}
    >
      <datalist id={NAMES_LIST}>
        {services.map((name) => (
          <option key={name} value={name} />
        ))}
      </datalist>
      <ListRows
        name="aliases"
        rows={aliases}
        onChange={draft.set}
        blank={() => ({ aliasNamespace: '', alias: '', namespace: '', service: '' })}
        addLabel={t('org_settings.aliases.add')}
        empty={t('org_settings.aliases.empty')}
        disabled={access.disabled}
        head={
          <>
            <span className="min-w-0 flex-[2] truncate">{t('org_settings.aliases.alias_namespace')}</span>
            <span className="min-w-0 flex-[3] truncate">{t('org_settings.aliases.alias')}</span>
            <span className="w-3.5 shrink-0" />
            <span className="min-w-0 flex-[2] truncate">{t('org_settings.aliases.namespace')}</span>
            <span className="min-w-0 flex-[3] truncate">{t('org_settings.aliases.service')}</span>
          </>
        }
        render={(alias, change, index) => (
          <>
            <input
              aria-label={t('org_settings.aliases.alias_namespace')}
              placeholder="default"
              value={alias.aliasNamespace}
              disabled={access.disabled}
              onChange={(event) => change({ ...alias, aliasNamespace: event.target.value })}
              className={field(index, false)}
            />
            <input
              aria-label={t('org_settings.aliases.alias')}
              placeholder={t('org_settings.aliases.alias')}
              list={NAMES_LIST}
              value={alias.alias}
              disabled={access.disabled}
              onChange={(event) => change({ ...alias, alias: event.target.value })}
              className={field(index, true)}
            />
            <ArrowRight aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
            <input
              aria-label={t('org_settings.aliases.namespace')}
              placeholder="default"
              value={alias.namespace}
              disabled={access.disabled}
              onChange={(event) => change({ ...alias, namespace: event.target.value })}
              className={field(index, false)}
            />
            <input
              aria-label={t('org_settings.aliases.service')}
              placeholder={t('org_settings.aliases.service')}
              list={NAMES_LIST}
              value={alias.service}
              disabled={access.disabled}
              onChange={(event) => change({ ...alias, service: event.target.value })}
              className={field(index, true)}
            />
          </>
        )}
      />
      <Issues issues={said} />
    </ApmCard>
  );
}

/** What the catalog asks of a service by how much it matters: a team, an owner, a repository. */
export function CatalogRulesCard({
  draft,
  access,
  pending,
  onSave,
}: {
  draft: Draft<MetadataRules>;
  access: ActionAccess;
  pending: boolean;
  onSave: () => void;
}) {
  const { t } = useTranslation('apm');
  const rules = draft.value;
  const options = [{ value: NONE, label: t('org_settings.catalog_rules.never') }, ...TIERS.map((tier) => ({ value: tier, label: t('org_settings.catalog_rules.from_tier', { tier: tier.toUpperCase() }) }))];
  const keys = [
    ['team_from_tier', 'teamFromTier'],
    ['owner_from_tier', 'ownerFromTier'],
    ['repository_from_tier', 'repositoryFromTier'],
  ] as const;

  return (
    <ApmCard
      icon={ClipboardCheck}
      tone="text-[var(--orange-ink)]"
      title={t('org_settings.catalog_rules.title')}
      description={t('org_settings.catalog_rules.hint')}
      testId="settings-catalog-rules"
      actions={<CardSave name="catalog-rules" dirty={draft.dirty} invalid={false} pending={pending} access={access} onSave={onSave} onReset={draft.reset} />}
    >
      <div className="grid grid-cols-[120px_minmax(0,1fr)] items-center gap-x-3 gap-y-2 p-3 font-sans text-xs">
        {keys.map(([key, field]) => (
          <div key={key} className="contents">
            <span className="font-strong text-tx-1">{t(`org_settings.catalog_rules.${key}`)}</span>
            <span>
              <FittedSelect
                aria-label={t(`org_settings.catalog_rules.${key}`)}
                value={rules[field] ?? NONE}
                onValueChange={(value) => draft.set({ ...rules, [field]: value === NONE ? null : (value as ServiceTier) })}
                options={options}
                maxWidth={200}
                disabled={access.disabled}
                className="h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 hover:bg-bg-3"
              />
            </span>
          </div>
        ))}
      </div>
    </ApmCard>
  );
}
