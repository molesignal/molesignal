import { ChevronRight, ChevronLeft } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useLocation } from 'react-router-dom';

import { findProductRoute, type ProductBreadcrumbItem } from '@/product/ia';
import { cn } from '@/shell/lib/cn';

interface PageHeaderProps {
  title: React.ReactNode;
  subtitle?: string | undefined;
  toolbar?: React.ReactNode | undefined;
  /**
   * Crumbs leading to this page. When omitted, PageHeader auto-derives
   * from the current route's `breadcrumbs` field in `ia.ts` — pass `null`
   * to explicitly suppress.
   */
  breadcrumbs?: readonly ProductBreadcrumbItem[] | null | undefined;
  /**
   * Optional back link, used on deep routes (e.g. `/dashboards/:id` → back
   * to `/dashboards`). When omitted, PageHeader auto-derives from
   * `ia.ts.backTo`. Pass `null` to suppress.
   */
  backTo?: string | null | undefined;
  className?: string | undefined;
}

/**
 * Page header — sits inside <main>, below the global Topbar.
 *
 * Three-band structure (Phase 3 IA spec):
 *   1. Breadcrumb + optional back-link
 *   2. Title (display-strong) + subtitle
 *   3. Right-aligned toolbar (filters / time picker / run button / etc.)
 *
 * Breadcrumbs are sourced from `ia.ts` so a deep route doesn't have to
 * repeat its crumb chain inline. Pass an explicit `breadcrumbs` prop to
 * override; pass `null` to suppress for landing pages.
 */
export function PageHeader({
  title,
  subtitle,
  toolbar,
  breadcrumbs,
  backTo,
  className,
}: PageHeaderProps) {
  const { t } = useTranslation('nav');
  const location = useLocation();
  const route = React.useMemo(() => findProductRoute(location.pathname), [location.pathname]);

  // Resolve breadcrumbs: explicit prop > route metadata > none.
  const resolvedCrumbs: readonly ProductBreadcrumbItem[] | undefined =
    breadcrumbs === null
      ? undefined
      : breadcrumbs ?? route?.breadcrumbs;
  const resolvedBackTo: string | undefined =
    backTo === null ? undefined : backTo ?? route?.backTo;

  const hasCrumbs = (resolvedCrumbs?.length ?? 0) > 0;
  const hasBack = !!resolvedBackTo;
  const hasNav = hasCrumbs || hasBack;

  // Publish the live header height as a CSS variable so page bodies can size
  // themselves with `calc(100vh - … - var(--pageheader-h))` instead of each
  // hardcoding an approximate value that drifts when density or content
  // changes. Reset to 0 on unmount so a headerless route inherits no stale
  // offset.
  const headerRef = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => {
    const el = headerRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const root = document.documentElement;
    const write = () =>
      root.style.setProperty('--pageheader-h', `${Math.round(el.getBoundingClientRect().height)}px`);
    write();
    const ro = new ResizeObserver(write);
    ro.observe(el);
    return () => {
      ro.disconnect();
      root.style.setProperty('--pageheader-h', '0px');
    };
  }, []);

  return (
    <div
      ref={headerRef}
      className={cn(
        'flex flex-col gap-3 border-b border-bd-0 bg-bg-1 px-6 py-5',
        className,
      )}
    >
      {hasNav && (
        <div className="flex min-w-0 items-center gap-2 font-sans text-xs font-strong text-tx-2">
          {hasBack && (
            <Link
              to={resolvedBackTo!}
              className={cn(
                'flex items-center gap-1 rounded text-tx-2 hover:text-tx-0',
                'transition-colors duration-fast ease-default',
                'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
              )}
              aria-label={t('breadcrumbs.back', { defaultValue: 'Back' })}
            >
              <ChevronLeft className="h-3.5 w-3.5" />
              <span className="hidden sm:inline">{t('breadcrumbs.back', { defaultValue: 'Back' })}</span>
            </Link>
          )}
          {hasBack && hasCrumbs && <span aria-hidden className="h-3 w-px bg-bd-1" />}
          {hasCrumbs && <Breadcrumbs items={resolvedCrumbs!} />}
        </div>
      )}
      <div className="flex min-w-0 flex-wrap items-end gap-4 xl:flex-nowrap xl:gap-5">
        <div className="min-w-[240px] flex-1">
          <div className="type-page-title font-sans font-display-strong tracking-[-0.025em] text-tx-0">{title}</div>
          {subtitle && <div className="mt-1 max-w-3xl truncate text-sm text-tx-2">{subtitle}</div>}
        </div>
        {toolbar && <div className="ml-auto flex max-w-full flex-wrap items-center justify-end gap-2">{toolbar}</div>}
      </div>
    </div>
  );
}

function Breadcrumbs({ items }: { items: readonly ProductBreadcrumbItem[] }) {
  const { t } = useTranslation('nav');
  return (
    <nav aria-label={t('breadcrumbs.label', { defaultValue: 'Breadcrumb' })} className="min-w-0">
      <ol className="flex min-w-0 items-center gap-1.5">
        {items.map((item, i) => {
          const isLast = i === items.length - 1;
          const label = item.label ?? t(item.labelKey, { defaultValue: item.labelKey });
          return (
            <React.Fragment key={`${item.labelKey}-${i}`}>
              <li className="min-w-0 truncate">
                {item.to && !isLast ? (
                  <Link
                    to={item.to}
                    className={cn(
                      'rounded text-tx-2 hover:text-tx-0',
                      'transition-colors duration-fast ease-default',
                      'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
                    )}
                  >
                    {label}
                  </Link>
                ) : (
                  <span
                    aria-current={isLast ? 'page' : undefined}
                    className={isLast ? 'text-tx-1' : 'text-tx-2'}
                  >
                    {label}
                  </span>
                )}
              </li>
              {!isLast && (
                <li aria-hidden className="text-tx-3">
                  <ChevronRight className="h-3.5 w-3.5" />
                </li>
              )}
            </React.Fragment>
          );
        })}
      </ol>
    </nav>
  );
}

interface PageBodyProps {
  children?: React.ReactNode | undefined;
  className?: string | undefined;
  padded?: boolean | undefined;
}

export function PageBody({ children, className, padded = true }: PageBodyProps) {
  return (
    <div
      className={cn(
        // PageHeader publishes its live height as --pageheader-h (see above);
        // falls back to 0px when a route renders no header, so the body always
        // reaches the viewport edge without an awkward dead zone.
        'min-h-[calc(100vh-var(--topbar-h)-var(--pageheader-h,0px)-var(--contextbar-h,0px))] bg-bg-0',
        padded && 'p-6',
        className,
      )}
    >
      {children}
    </div>
  );
}
