import { useQuery } from '@tanstack/react-query';
import { Building2, CircleAlert, Users } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import {
  type KnowledgeBase,
  type KnowledgeScope,
  knowledgeOverview,
  scopeKey,
} from '@/api/agent/knowledge';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { cn } from '@/shell/lib/cn';

import { KnowledgeExplorer } from './KnowledgeExplorer';

/** A size in the units people read: B, KiB, MiB. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}

/**
 * The knowledge of the organization and of every team the person can see: one
 * base each, with its files, whether it has the entry file MOLE.md, and whether
 * that file was written before the latest changes to the rest. A link can name
 * the base and the file to open: `?base=team:<id>&path=runbooks/api.md`.
 */
export function KnowledgeView() {
  const { t } = useTranslation('agent');
  const [params] = useSearchParams();
  const overview = useQuery({
    queryKey: ['agent', 'knowledge'],
    queryFn: knowledgeOverview,
    retry: false,
  });
  const [selected, setSelected] = React.useState<string>(() => params.get('base') ?? 'org');
  const linkedPath = params.get('path');

  if (overview.isLoading) return <ProductState variant="loading" />;
  if (overview.isError) return <ProductState variant="error" error={overview.error} />;
  const data = overview.data;
  if (!data) return null;

  const current = data.bases.find((base) => scopeKey(base) === selected) ?? data.bases[0];

  return (
    <div data-agent-knowledge className="flex h-full min-h-0 flex-col md:flex-row">
      <nav
        aria-label={t('knowledge.bases_label')}
        className="shrink-0 overflow-auto border-b border-bd-0 p-2 md:w-[220px] md:border-b-0 md:border-r"
      >
        <ul className="flex flex-col gap-0.5">
          {data.bases.map((base) => (
            <li key={scopeKey(base)}>
              <BaseRow
                base={base}
                active={current ? scopeKey(current) === scopeKey(base) : false}
                onSelect={() => setSelected(scopeKey(base))}
              />
            </li>
          ))}
        </ul>
      </nav>
      <div className="min-h-0 min-w-0 flex-1 overflow-hidden">
        {current && (
          <KnowledgeExplorer
            key={scopeKey(current)}
            base={current}
            initialPath={linkedPath}
            limits={data.limits}
            scope={{ scope: current.scope, team_id: current.team_id ?? null } satisfies KnowledgeScope}
          />
        )}
      </div>
    </div>
  );
}

/**
 * A base in the list, one line like the files beside it: its name and how many
 * files it has, with a mark when MOLE.md is missing or older than the rest.
 */
function BaseRow({
  base,
  active,
  onSelect,
}: {
  base: KnowledgeBase;
  active: boolean;
  onSelect: () => void;
}) {
  const { t } = useTranslation('agent');
  const Icon = base.scope === 'org' ? Building2 : Users;
  const name = base.scope === 'org' ? t('knowledge.org') : (base.team_name ?? base.team_id ?? '');
  const warning =
    base.changed_since_entry > 0
      ? t('knowledge.stale_short', { count: base.changed_since_entry })
      : !base.entry && base.files > 0
        ? t('knowledge.entry_missing')
        : null;
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={active ? 'true' : undefined}
      data-agent-knowledge-base={scopeKey(base)}
      title={
        base.updated_at_micros
          ? t('knowledge.updated', { time: formatMicrosActive(base.updated_at_micros) })
          : undefined
      }
      className={cn(
        'flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-left',
        active ? 'bg-bg-2 text-tx-0' : 'text-tx-1 hover:bg-bg-2',
      )}
    >
      <Icon aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-indigo" />
      <span className="min-w-0 flex-1 truncate text-xs font-strong">{name}</span>
      {warning && (
        <span
          data-agent-knowledge-stale={base.changed_since_entry > 0 ? '' : undefined}
          title={warning}
          className="shrink-0 text-[var(--yellow-ink)]"
        >
          <CircleAlert aria-hidden="true" className="h-3.5 w-3.5" />
          <span className="sr-only">{warning}</span>
        </span>
      )}
      <span className="type-micro shrink-0 tabular-nums text-tx-3">
        {t('knowledge.file_count', { count: base.files })}
      </span>
    </button>
  );
}
