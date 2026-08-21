import type * as React from 'react';

import type { KpiStripItem } from '@/admin';
import { ProductState, type ProductStateProps } from '@/product/states';
import { uiLabelClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import {
  surfacePageRootClass,
  SurfacePageBody,
  SurfacePageHeader,
} from '@/shell/SurfaceWorkbench';

import type { ReportTab } from '../reportTypes';

interface ReportsPageTab {
  id: ReportTab;
  label: string;
  count?: number | undefined;
}

export function ReportsPageCanvas({
  title,
  subtitle,
  toolbar,
  kpis,
  tabs,
  activeTab,
  onTabChange,
  tabAction,
  actionBar,
  state,
  children,
}: {
  title: React.ReactNode;
  subtitle?: string | undefined;
  toolbar?: React.ReactNode | undefined;
  kpis?: readonly KpiStripItem[] | undefined;
  tabs: readonly ReportsPageTab[];
  activeTab: ReportTab;
  onTabChange: (tab: ReportTab) => void;
  tabAction?: React.ReactNode | undefined;
  actionBar?: React.ReactNode | undefined;
  state?: ProductStateProps | null | undefined;
  children?: React.ReactNode | undefined;
}) {
  return (
    <div data-page-appearance="surface" className={surfacePageRootClass}>
      <SurfacePageHeader title={title} subtitle={subtitle} toolbar={toolbar} />
      <SurfacePageBody>
        <main className="mx-auto w-full max-w-[2200px] space-y-[12px] bg-[var(--page-canvas)]">
          {kpis && kpis.length > 0 && <ReportsKpiStrip items={kpis} />}

          <section
            data-reports-workspace
            className="min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]"
            aria-label={title as string}
          >
            <header
              data-reports-navigation
              className="flex min-h-11 min-w-0 items-end"
            >
              <div
                role="tablist"
                aria-label={title as string}
                className="flex min-w-0 flex-1 items-end overflow-x-auto px-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
              >
                {tabs.map((tab) => {
                  const active = activeTab === tab.id;
                  return (
                    <button
                      key={tab.id}
                      type="button"
                      role="tab"
                      aria-selected={active}
                      onClick={() => onTabChange(tab.id)}
                      className={cn(
                        'flex min-h-11 shrink-0 items-center gap-2 border-b-[3px] px-3 py-2 font-sans text-sm font-strong transition-colors focus-visible:bg-bg-2 focus-visible:outline-none',
                        active
                          ? 'border-indigo text-tx-0'
                          : 'border-transparent text-tx-2 hover:text-tx-0',
                      )}
                    >
                      {tab.label}
                      {tab.count !== undefined && (
                        <span className="grid min-h-5 min-w-5 place-items-center rounded-full bg-bg-2 px-1.5 font-sans text-xs text-tx-2">
                          {tab.count}
                        </span>
                      )}
                    </button>
                  );
                })}
              </div>
              {tabAction && (
                <div className="ml-auto flex min-h-11 shrink-0 items-center px-3 py-1.5">
                  {tabAction}
                </div>
              )}
            </header>

            {actionBar && (
              <div className="flex min-h-12 min-w-0 flex-wrap items-center gap-2 px-4 py-2">
                {actionBar}
              </div>
            )}

            <div className="min-w-0">
              {state ? (
                <ProductState
                  {...state}
                  className={cn(
                    'rounded-none border-0 bg-transparent shadow-none',
                    state.className,
                  )}
                />
              ) : (
                children
              )}
            </div>
          </section>
        </main>
      </SurfacePageBody>
    </div>
  );
}

function ReportsKpiStrip({ items }: { items: readonly KpiStripItem[] }) {
  return (
    <section
      data-reports-kpis
      className="grid grid-cols-4 gap-[12px]"
    >
      {items.map((item, index) => (
        <div
          key={index}
          className="min-h-[92px] min-w-0 rounded-md bg-[var(--functional-surface)] px-4 py-3 [box-shadow:var(--shadow-functional-surface)]"
        >
          <div className={uiLabelClass}>{item.label}</div>
          <div
            className={cn(
              'mt-2 truncate font-sans text-2xl font-display-strong leading-none tracking-[-0.025em] tabular-nums',
              item.tone === 'good' && 'text-green',
              item.tone === 'warn' && 'text-yellow',
              item.tone === 'danger' && 'text-red',
              (!item.tone || item.tone === 'neutral') && 'text-tx-0',
            )}
          >
            {item.value}
          </div>
          {item.sub && (
            <div className="mt-1.5 truncate font-sans text-xs text-tx-2">
              {item.sub}
            </div>
          )}
        </div>
      ))}
    </section>
  );
}
