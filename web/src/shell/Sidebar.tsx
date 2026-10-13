import { GripVertical, Pin } from 'lucide-react';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useLocation } from 'react-router-dom';

import {
  accessibleProductNavigation,
  canAccessProductRoute,
  useProductAccess,
} from '@/product/access';
import {
  getProductRouteById,
  isProductRouteActive,
  PRODUCT_NAV_GROUP_META,
  PRODUCT_NAV_GROUPS,
  type ProductNavGroup,
  type ProductRouteMeta,
} from '@/product/ia';
import { cn } from '@/shell/lib/cn';
import { NavIndicator, useNavIndicator } from '@/shell/NavIndicator';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';
import { useSidebarStore } from '@/stores/useSidebarStore';

interface SidebarProps {
  collapsed: boolean;
  mobileOpen?: boolean | undefined;
  onNavigate?: (() => void) | undefined;
  onHoverChange?: ((hovered: boolean) => void) | undefined;
}

export function Sidebar({
  collapsed,
  mobileOpen = false,
  onNavigate,
  onHoverChange,
}: SidebarProps) {
  const { t } = useTranslation('nav');
  const { pathname } = useLocation();
  const visuallyCollapsed = collapsed && !mobileOpen;
  const access = useProductAccess();
  const navRef = useRef<HTMLElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);

  const pinned = useSidebarStore((s) => s.pinned);
  const togglePin = useSidebarStore((s) => s.togglePin);
  const unpin = useSidebarStore((s) => s.unpin);
  const reorderPinned = useSidebarStore((s) => s.reorderPinned);
  const [dragId, setDragId] = useState<string | null>(null);
  const [overId, setOverId] = useState<string | null>(null);

  const indicator = useNavIndicator(
    navRef,
    contentRef,
    `${pathname}|${visuallyCollapsed}|${pinned.join(',')}`,
  );

  const pinnedSet = new Set(pinned);
  const pinnedRoutes = pinned
    .map(getProductRouteById)
    .filter(
      (route): route is ProductRouteMeta =>
        !!route && canAccessProductRoute(route, access),
    );
  // Render one nav group, minus any items currently pinned (those live in the
  // Pinned section instead, so nothing shows twice). Returns null when the
  // group has nothing left to show — e.g. when Home itself is pinned.
  const renderGroup = (group: ProductNavGroup) => {
    const groupItems = accessibleProductNavigation(access, group).filter(
      (item) => !pinnedSet.has(item.id),
    );
    if (groupItems.length === 0) return null;
    const groupMeta = PRODUCT_NAV_GROUP_META[group];
    return (
      <div key={group} className="mb-1">
        {!visuallyCollapsed && group !== 'home' && (
          // Home is a stand-alone top item, so it has no "HOME" label. Every other
          // group is a block: a hairline closes the one above, the label names this one.
          <>
            <div aria-hidden className="mx-[22px] mb-0.5 mt-2 h-px bg-bd-0" />
            <GroupLabel current={groupItems.some((item) => isProductRouteActive(item, pathname))}>
              {t(groupMeta.labelKey)}
            </GroupLabel>
          </>
        )}
        {visuallyCollapsed && group !== 'home' && (
          // Without labels the rail is a column of icons; a hairline chunks it
          // into the same groups the expanded sidebar names.
          <div aria-hidden className="mx-auto mb-1.5 mt-1 h-px w-5 bg-bd-1" />
        )}
        <div className="flex flex-col gap-0.5 px-[12px]">
          {groupItems.map((item) => (
            <NavRow
              key={item.id}
              item={item}
              collapsed={visuallyCollapsed}
              onNavigate={onNavigate}
              pinControl={visuallyCollapsed ? null : { pinned: false, onToggle: () => togglePin(item.id) }}
            />
          ))}
        </div>
      </div>
    );
  };

  return (
    <aside
      aria-label={t('primary_navigation')}
      data-testid="primary-sidebar"
      onMouseEnter={() => onHoverChange?.(true)}
      onMouseLeave={() => onHoverChange?.(false)}
      className={cn(
        'fixed bottom-0 left-0 top-topbar z-40 flex w-sidebar flex-col',
        'border-0 bg-[var(--sidebar-surface)]',
        'transition-[transform,width] duration-150 ease-out-default',
        mobileOpen ? 'relative inset-auto h-full w-full translate-x-0' : '-translate-x-full md:translate-x-0',
        visuallyCollapsed ? 'md:w-sidebar-collapsed' : 'md:w-sidebar',
      )}
    >
      <nav ref={navRef} className="relative flex-1 overflow-y-auto py-2">
        <NavIndicator
          box={indicator.box}
          animated={indicator.animated}
          testId="sidebar-active-indicator"
        />
        <div ref={contentRef}>
          {renderGroup('home')}

          {/* Pinned items render here ONLY — renderGroup drops them from their
              home group so nothing shows twice; unpinning returns an item to its
              original group slot. The section disappears when it is empty. */}
          {!visuallyCollapsed && pinnedRoutes.length > 0 && (
            <MiniSection
              labelKey="pinned"
              current={pinnedRoutes.some((route) => isProductRouteActive(route, pathname))}
            >
              {pinnedRoutes.map((route) => (
                <MiniNavRow
                  key={route.id}
                  route={route}
                  onNavigate={onNavigate}
                  action={{ kind: 'unpin', onToggle: () => unpin(route.id) }}
                  drag={{
                    dragging: dragId === route.id,
                    over: overId === route.id && dragId !== null && dragId !== route.id,
                    onDragStart: () => setDragId(route.id),
                    onDragEnter: () => setOverId(route.id),
                    onDrop: () => {
                      if (dragId && dragId !== route.id) reorderPinned(dragId, route.id);
                      setDragId(null);
                      setOverId(null);
                    },
                    onDragEnd: () => {
                      setDragId(null);
                      setOverId(null);
                    },
                  }}
                />
              ))}
            </MiniSection>
          )}
          {PRODUCT_NAV_GROUPS.filter((group) => group !== 'home').map((group) => renderGroup(group))}
        </div>
      </nav>
    </aside>
  );
}

function MiniSection({
  labelKey,
  current,
  children,
}: {
  labelKey: string;
  current: boolean;
  children: ReactNode;
}) {
  const { t } = useTranslation('nav');
  return (
    <div className="mb-1">
      <div aria-hidden className="mx-[22px] mb-0.5 mt-2 h-px bg-bd-0" />
      <GroupLabel current={current}>{t(labelKey)}</GroupLabel>
      <div className="flex flex-col gap-0.5 px-[12px]">{children}</div>
    </div>
  );
}

/**
 * The name of a block of destinations: quiet small capitals, one tier above the
 * rows it heads. The block that holds the current page names itself a tier darker.
 */
function GroupLabel({ children, current = false }: { children: ReactNode; current?: boolean }) {
  return (
    <div
      className={cn(
        'font-sidebar-face type-eyebrow px-[22px] pb-1 pt-1.5 transition-colors duration-fast',
        current ? 'text-tx-0' : 'text-tx-2',
      )}
    >
      {children}
    </div>
  );
}

interface MiniRowDrag {
  dragging: boolean;
  over: boolean;
  onDragStart: () => void;
  onDragEnter: () => void;
  onDrop: () => void;
  onDragEnd: () => void;
}

/** Row for the Pinned section. Shares the fixed-group row metrics
 *  (`h-sidebar-item`, `text-xs`, 16px icon, `text-tx-1`) so the whole sidebar
 *  keeps one density — only the trailing pin/grip controls differ. */
function MiniNavRow({
  route,
  action,
  onNavigate,
  drag,
}: {
  route: ProductRouteMeta;
  action: { kind: 'pin' | 'unpin'; onToggle: () => void };
  onNavigate?: (() => void) | undefined;
  drag?: MiniRowDrag | undefined;
}) {
  const { t } = useTranslation('nav');
  const { pathname } = useLocation();
  const active = isProductRouteActive(route, pathname);
  const label = t(route.labelKey);
  const actionLabel = action.kind === 'unpin' ? t('unpin') : t('pin');
  // Drag is allowed only when it begins on the grip handle: the handle's
  // pointer-down arms this ref; onDragStart cancels any drag that isn't armed
  // (so dragging the label or pin button does nothing).
  const armedRef = useRef(false);
  return (
    <div
      className={cn(
        'group/mini relative rounded-md',
        // Drop-target hint: a 2px indigo rule along the top of the hovered row.
        drag?.over &&
          'before:absolute before:inset-x-1 before:-top-px before:z-10 before:h-0.5 before:rounded-full before:bg-indigo',
        drag?.dragging && 'opacity-40',
      )}
      draggable={drag ? true : undefined}
      onDragStart={
        drag
          ? (e) => {
              if (!armedRef.current) {
                e.preventDefault();
                return;
              }
              e.dataTransfer.effectAllowed = 'move';
              // Firefox won't start a drag unless some data is set.
              e.dataTransfer.setData('text/plain', route.id);
              drag.onDragStart();
            }
          : undefined
      }
      onDragEnter={drag ? () => drag.onDragEnter() : undefined}
      onDragOver={
        drag
          ? (e) => {
              e.preventDefault();
              e.dataTransfer.dropEffect = 'move';
            }
          : undefined
      }
      onDrop={
        drag
          ? (e) => {
              e.preventDefault();
              drag.onDrop();
            }
          : undefined
      }
      onDragEnd={
        drag
          ? () => {
              armedRef.current = false;
              drag.onDragEnd();
            }
          : undefined
      }
    >
      <Link
        to={route.path}
        onClick={onNavigate}
        draggable={false}
        aria-current={active ? 'page' : undefined}
        className={`${cn(
          // Match NavRow metrics (h-sidebar-item / text-xs / text-tx-1) so the
          // Pinned rows share the fixed groups' density.
          'group relative flex h-sidebar-item items-center gap-2 rounded-md pl-[10px] text-xs text-tx-1',
          // extra right padding for the grip + pin controls (grip only on pinned rows)
          drag ? 'pr-16' : 'pr-9',
          'transition-colors duration-fast ease-default hover:bg-bg-3 hover:text-tx-0',
          'focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none',
          active &&
            'bg-indigo-dim font-strong text-indigo-soft hover:bg-indigo-dim hover:text-indigo-soft',
        )} font-sidebar-face`}
      >
        <route.icon
          strokeWidth={1.75}
          className="h-4 w-4 shrink-0 text-tx-2 transition-colors duration-fast group-hover:text-tx-0 group-aria-[current=page]:text-indigo-soft"
        />
        <span className="flex-1 truncate">{label}</span>
      </Link>
      <div className="absolute right-0.5 top-1/2 flex -translate-y-1/2 items-center gap-0.5">
        {drag && (
          <button
            type="button"
            aria-label={t('drag_handle')}
            title={t('drag_handle')}
            // Pointer-down on the grip arms the drag (see armedRef); only a
            // gesture that starts here reorders, not the label or pin button.
            onMouseDown={() => {
              armedRef.current = true;
            }}
            onMouseUp={() => {
              armedRef.current = false;
            }}
            className="grid h-7 w-7 cursor-grab place-items-center rounded text-tx-3 opacity-0 transition-opacity hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:opacity-100 focus-visible:outline-none active:cursor-grabbing group-hover/mini:opacity-100 group-focus-within/mini:opacity-100"
          >
            <GripVertical className="h-3 w-3" />
          </button>
        )}
        <button
          type="button"
          onClick={action.onToggle}
          aria-label={actionLabel}
          title={actionLabel}
          className={cn(
            'grid h-7 w-7 place-items-center rounded text-tx-3 opacity-0 transition-opacity',
            'hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none',
            'focus-visible:opacity-100 group-hover/mini:opacity-100 group-focus-within/mini:opacity-100',
            action.kind === 'unpin' && 'text-indigo-soft',
          )}
        >
          <Pin className={cn('h-3 w-3', action.kind === 'unpin' && 'fill-current')} />
        </button>
      </div>
    </div>
  );
}

function NavRow({
  item,
  collapsed,
  onNavigate,
  pinControl,
}: {
  item: ProductRouteMeta;
  collapsed: boolean;
  onNavigate?: (() => void) | undefined;
  pinControl?: { pinned: boolean; onToggle: () => void } | null;
}) {
  const { t } = useTranslation('nav');
  const { pathname } = useLocation();
  const active = isProductRouteActive(item, pathname);
  const label = t(item.labelKey);
  const [tooltipOpen, setTooltipOpen] = useState(false);

  useEffect(() => {
    if (!collapsed) setTooltipOpen(false);
  }, [collapsed]);

  const link = (
    <Link
      to={item.path}
      onClick={onNavigate}
      aria-label={collapsed ? label : undefined}
      aria-current={active ? 'page' : undefined}
      className={`${cn(
        // Shell navigation stays stable across density modes. Rows are regular
        // weight so the one that is current is the one that stands out.
        'group relative flex h-sidebar-item items-center gap-2 rounded-md pl-[10px] pr-2 text-xs text-tx-1',
        'transition-colors duration-fast ease-default',
        'hover:bg-bg-3 hover:text-tx-0',
        'focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none',
        active && 'bg-indigo-dim font-strong text-indigo-soft hover:bg-indigo-dim hover:text-indigo-soft',
        collapsed && 'justify-center pl-0 pr-0',
        !collapsed && pinControl && 'pr-10',
      )} font-sidebar-face`}
    >
      <item.icon
        strokeWidth={1.75}
        className="h-4 w-4 shrink-0 text-tx-2 transition-colors duration-fast group-hover:text-tx-0 group-aria-[current=page]:text-indigo-soft"
      />
      {!collapsed && <span className="flex-1 truncate">{label}</span>}
    </Link>
  );

  const actionLabel = pinControl
    ? pinControl.pinned
      ? t('unpin')
      : t('pin')
    : null;

  // Keep both the wrapper and TooltipTrigger mounted while hover temporarily
  // expands the compact rail. Swapping a bare NavLink for a tooltip-wrapped
  // NavLink detached the anchor between pointerdown and click, so the first
  // navigation attempt was lost.
  return (
    <Tooltip
      open={collapsed && tooltipOpen}
      onOpenChange={(open) => setTooltipOpen(collapsed && open)}
    >
      <TooltipTrigger asChild>
        <div
          className="group/nav relative"
          onPointerLeave={() => setTooltipOpen(false)}
        >
          {link}
          {pinControl && actionLabel && (
            <button
              type="button"
              onClick={pinControl.onToggle}
              aria-label={actionLabel}
              title={actionLabel}
              className={cn(
                'absolute right-1 top-1/2 grid h-7 w-7 -translate-y-1/2 place-items-center rounded text-tx-3 opacity-0 transition-opacity',
                'hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none',
                'focus-visible:opacity-100 group-hover/nav:opacity-100 group-focus-within/nav:opacity-100',
                pinControl.pinned && 'text-indigo-soft',
              )}
            >
              <Pin className={cn('h-3 w-3', pinControl.pinned && 'fill-current')} />
            </button>
          )}
        </div>
      </TooltipTrigger>
      {collapsed && <TooltipContent side="right">{label}</TooltipContent>}
    </Tooltip>
  );
}
