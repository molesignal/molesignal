import { ArrowLeft, ArrowRight, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { MapNode } from '@/api/apmMap';
import { cn } from '@/shell/lib/cn';

import { formatRate } from '../format';
import type { DrawnEdge, GroupNode } from './graph';

/** The most calls the card lists each way. */
const LISTED = 6;

const ACTION = 'inline-flex h-7 items-center gap-1 rounded-md bg-[var(--control-surface)] px-2.5 font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3';

/**
 * The node chosen on the map, over its corner: what it is, what it served
 * or was asked, how many nodes call into it and how many it reaches, its
 * calls each way (each opening in the call drawer), and where it leads: a
 * service's page, a folded namespace's services.
 */
export function ChosenCard({
  node,
  group,
  figures,
  callers,
  callees,
  calls,
  nameOf,
  onOpenCall,
  onOpenService,
  onUnfold,
  onClose,
}: {
  node: MapNode | undefined;
  group: GroupNode | undefined;
  figures: string;
  callers: number;
  callees: number;
  /** The calls in and out of the node as drawn. */
  calls: readonly DrawnEdge[];
  nameOf: (id: string) => string;
  onOpenCall: (call: DrawnEdge) => void;
  onOpenService: (node: MapNode) => void;
  onUnfold: (namespace: string) => void;
  onClose: () => void;
}) {
  const id = node?.id ?? group?.id;
  const listed = [...calls].sort((left, right) => right.red.requestCount - left.red.requestCount).slice(0, LISTED * 2);
  const { t } = useTranslation('apm');
  const what = group ? t('map.kind.group') : node?.kind === 'external' ? t(`map.category.${node.category ?? 'other'}`) : t(`map.kind.${node?.kind ?? 'service'}`);

  return (
    <section
      data-apm-map-chosen={node?.id ?? group?.id}
      aria-label={node?.name ?? group?.namespace}
      className="absolute right-3 top-3 z-10 w-[280px] rounded-md bg-bg-1 p-3 font-sans text-xs [box-shadow:var(--shadow-functional-surface)]"
    >
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-strong text-tx-0">{node?.name ?? group?.namespace}</div>
          <div className="mt-0.5 text-tx-2">{group ? t('map.group_figures', { count: group.services }) : what}</div>
        </div>
        <button type="button" aria-label={t('map.close_chosen')} onClick={onClose} className="grid h-6 w-6 shrink-0 place-items-center rounded text-tx-3 outline-none hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3">
          <X aria-hidden className="h-3.5 w-3.5" />
        </button>
      </div>
      <p className="mt-2 tabular-nums text-tx-1">{figures}</p>
      <p className="mt-1 text-tx-2">{t('map.reach', { callers, callees })}</p>
      {listed.length > 0 && (
        <ul aria-label={t('map.calls')} className="mt-2 border-t border-bd-0 pt-1">
          {listed.map((call) => {
            const out = call.source === id;
            const Arrow = out ? ArrowRight : ArrowLeft;
            return (
              <li key={call.id}>
                <button
                  type="button"
                  data-apm-map-call={call.id}
                  onClick={() => onOpenCall(call)}
                  className="flex h-7 w-full items-center gap-1.5 rounded px-1 text-left outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
                >
                  <Arrow aria-label={t(out ? 'map.calls_out' : 'map.calls_in')} className="h-3 w-3 shrink-0 text-tx-3" />
                  <span className={cn('truncate text-tx-0', call.change === 'gone' && 'text-tx-3 line-through')}>{nameOf(out ? call.target : call.source)}</span>
                  <span className={cn('ml-auto shrink-0 tabular-nums', call.red.errorCount > 0 ? 'text-[var(--red-ink)]' : 'text-tx-3')}>{formatRate(call.red.errorRate)}</span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {(node?.kind === 'service' && node.health) || group ? (
        <div className="mt-3 flex justify-end">
          {group ? (
            <button type="button" className={ACTION} onClick={() => onUnfold(group.namespace)} data-apm-map-unfold="">
              {t('map.unfold')}
            </button>
          ) : (
            <button type="button" className={ACTION} onClick={() => node && onOpenService(node)} data-apm-map-open-service="">
              {t('map.open_service')}
              <ArrowRight aria-hidden className="h-3 w-3" />
            </button>
          )}
        </div>
      ) : null}
    </section>
  );
}
