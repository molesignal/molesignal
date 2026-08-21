import { ChevronRight, ChevronLeft, type LucideIcon } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useLocation } from 'react-router-dom';

import {
  findProductRoute,
  PRODUCT_NAV_ITEMS,
  type ProductBreadcrumbItem,
} from '@/product/ia';
import { cn } from '@/shell/lib/cn';
import { PageTitleRow } from '@/shell/PageTitleRow';

interface PageHeaderProps {
  title: React.ReactNode;
  subtitle?: string | undefined;
  toolbar?: React.ReactNode | undefined;
  /**
   * Kept for backwards compatibility. Every PageHeader now uses the same
   * single-line title rhythm, regardless of module or icon.
   */
  compact?: boolean | undefined;
  /** Optional icon for a management or custom module header. */
  moduleIcon?: LucideIcon | null | undefined;
  /** Stable test hook for custom module headers that share this primitive. */
  moduleIconTestId?: string | undefined;
  /**
   * Resource drill-down crumbs leading to this page. When omitted,
   * PageHeader auto-derives from the current route's `breadcrumbs` field in
   * `ia.ts`. Same-level module pages should not define crumbs; pass `null` to
   * explicitly suppress them in a shared tabbed layout.
   */
  breadcrumbs?: readonly ProductBreadcrumbItem[] | null | undefined;
  /**
   * Optional back link for an isolated workspace that has no breadcrumbs.
   * When omitted, PageHeader auto-derives from `ia.ts.backTo`. Breadcrumbs
   * always take precedence so both navigation models never render together.
   */
  backTo?: string | null | undefined;
  className?: string | undefined;
}

/**
 * Page header — sits inside <main>, below the global Topbar.
 *
 * Two-band structure:
 *   1. Optional resource breadcrumb or isolated-workspace back-link
 *   2. Fixed-height `Title · Description` row with an optional toolbar
 *
 * Breadcrumbs are sourced from `ia.ts` so a deep route doesn't have to repeat
 * its crumb chain inline. The sidebar already identifies the product module,
 * so chains deeper than two items omit that first module crumb. A one-item
 * chain is not navigation and is suppressed.
 */
export function PageHeader({
  title,
  subtitle,
  toolbar,
  compact = false,
  moduleIcon,
  moduleIconTestId = 'page-header-module-icon',
  breadcrumbs,
  backTo,
  className,
}: PageHeaderProps) {
  const { t } = useTranslation('nav');
  const location = useLocation();
  const route = React.useMemo(() => findProductRoute(location.pathname), [location.pathname]);
  const iconRoute = React.useMemo(() => {
    const ownerRoute = PRODUCT_NAV_ITEMS.find(
      (candidate) =>
        candidate.group === 'observe' && candidate.owner === route?.owner,
    );
    if (ownerRoute) return ownerRoute;
    const parentRoute = PRODUCT_NAV_ITEMS.find(
      (candidate) =>
        candidate.group === 'observe' &&
        (location.pathname === candidate.path ||
          location.pathname.startsWith(`${candidate.path}/`)),
    );
    return parentRoute ?? (route?.group === 'observe' ? route : undefined);
  }, [location.pathname, route]);
  const HeaderIcon = moduleIcon === null ? undefined : moduleIcon ?? iconRoute?.icon;
  const compactRequested = compact || Boolean(HeaderIcon);

  // Resolve breadcrumbs: explicit prop > route metadata > none. The sidebar
  // already carries module identity, so a deep chain starts at the first
  // module-internal level (for example Applications / checkout-web).
  const sourceCrumbs: readonly ProductBreadcrumbItem[] | undefined =
    breadcrumbs === null ? undefined : breadcrumbs ?? route?.breadcrumbs;
  const resolvedCrumbs = visibleBreadcrumbs(sourceCrumbs);
  const resolvedBackTo: string | undefined =
    backTo === null ? undefined : backTo ?? route?.backTo;

  const hasCrumbs = (resolvedCrumbs?.length ?? 0) > 0;
  // A breadcrumb and Back link express the same navigation relationship.
  // Breadcrumbs win; standalone Back remains available to explicit full-screen
  // workspaces through `breadcrumbs={null}` + `backTo="…"`.
  const hasBack = !hasCrumbs && !!resolvedBackTo;
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
      data-testid="page-header"
      data-page-header-layout="inline"
      data-page-header-compact={compactRequested ? 'true' : 'false'}
      className={cn(
        'flex flex-col gap-1.5 border-b border-bd-0 bg-bg-1 px-6 py-1.5',
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
                'focus-visible:bg-bg-2 focus-visible:text-tx-0',
              )}
              aria-label={t('breadcrumbs.back', { defaultValue: 'Back' })}
            >
              <ChevronLeft className="h-3.5 w-3.5" />
              <span className="hidden sm:inline">{t('breadcrumbs.back', { defaultValue: 'Back' })}</span>
            </Link>
          )}
          {hasCrumbs && <Breadcrumbs items={resolvedCrumbs!} />}
        </div>
      )}
      <PageTitleRow
        title={title}
        description={subtitle}
        leading={
          HeaderIcon ? (
            <span
              aria-hidden
              data-testid={moduleIconTestId}
              className="grid h-7 w-7 shrink-0 place-items-center rounded-md bg-indigo/10 text-indigo"
            >
              <HeaderIcon className="h-3.5 w-3.5" strokeWidth={1.8} />
            </span>
          ) : undefined
        }
        actions={toolbar}
      />
    </div>
  );
}

function visibleBreadcrumbs(
  items: readonly ProductBreadcrumbItem[] | undefined,
): readonly ProductBreadcrumbItem[] | undefined {
  if (!items || items.length < 2) return undefined;
  return items.length > 2 ? items.slice(1) : items;
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
                      'focus-visible:bg-bg-2 focus-visible:text-tx-0',
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
      data-page-body
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
