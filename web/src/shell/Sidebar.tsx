import { GripVertical, Pin } from 'lucide-react';
import { useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { NavLink } from 'react-router-dom';

import {
  accessibleProductNavigation,
  canAccessProductRoute,
  useProductAccess,
} from '@/product/access';
import {
  getProductRouteById,
  PRODUCT_NAV_GROUP_META,
  PRODUCT_NAV_GROUPS,
  type ProductNavGroup,
  type ProductRouteMeta,
} from '@/product/ia';
import { cn } from '@/shell/lib/cn';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';
import { useSidebarStore } from '@/stores/useSidebarStore';

interface SidebarProps {
  collapsed: boolean;
  mobileOpen?: boolean | undefined;
  onNavigate?: (() => void) | undefined;
}

export function Sidebar({ collapsed, mobileOpen = false, onNavigate }: SidebarProps) {
  const { t } = useTranslation('nav');
  const visuallyCollapsed = collapsed && !mobileOpen;
  const access = useProductAccess();

  const pinned = useSidebarStore((s) => s.pinned);
  const recent = useSidebarStore((s) => s.recent);
  const togglePin = useSidebarStore((s) => s.togglePin);
  const unpin = useSidebarStore((s) => s.unpin);
  const reorderPinned = useSidebarStore((s) => s.reorderPinned);
  const [dragId, setDragId] = useState<string | null>(null);
  const [overId, setOverId] = useState<string | null>(null);

  const pinnedSet = new Set(pinned);
  const pinnedRoutes = pinned
    .map(getProductRouteById)
    .filter(
      (route): route is ProductRouteMeta =>
        !!route && canAccessProductRoute(route, access),
    );
  // Ids that already have a permanent home in a fixed nav group. Recent must not
  // echo these — its whole purpose is quick access to pages that AREN'T already
  // one click away in the sidebar. Built from the same source the fixed groups
  // render (the DB-backed capability navigation), so the two never drift apart.
  const groupedIds = new Set(
    PRODUCT_NAV_GROUPS.flatMap((group) =>
      accessibleProductNavigation(access, group),
    ).map((route) => route.id),
  );
  // Recent excludes pinned (shown in the Pinned section) and anything already in
  // a fixed group, so the same destination never appears twice. Recomputed every
  // render, so the dedup also applies whenever the recent list updates.
  const recentRoutes = recent
    .map(getProductRouteById)
    .filter(
      (route): route is ProductRouteMeta =>
        !!route &&
        canAccessProductRoute(route, access) &&
        !pinnedSet.has(route.id) &&
        !groupedIds.has(route.id),
    )
    .slice(0, 4);

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
      <div key={group} className="mb-1.5">
        {!visuallyCollapsed && group !== 'home' && (
          // Group labels use the shell's micro role so they remain secondary.
          // Home is a stand-alone top item, so we skip a "HOME" label.
          <div className="font-sidebar-face type-micro px-3.5 pb-1 pt-2.5 font-semibold uppercase tracking-wide text-tx-3">
            {t(groupMeta.labelKey)}
          </div>
        )}
        <div className="flex flex-col gap-0.5 px-1.5">
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
      className={cn(
        'fixed bottom-0 left-0 top-topbar z-40 flex w-sidebar flex-col border-r border-bd-0 bg-bg-1',
        'transition-[transform,width] duration-normal ease-out-default',
        mobileOpen ? 'translate-x-0 shadow-lg' : '-translate-x-full md:translate-x-0',
        visuallyCollapsed ? 'md:w-sidebar-collapsed' : 'md:w-sidebar',
      )}
    >
      <nav className="flex-1 overflow-y-auto py-2">
        {renderGroup('home')}

        {/* Personalization. Pinned items render here ONLY — renderGroup drops
            them from their home group so nothing shows twice; unpinning returns
            an item to its original group slot. Recent is auto-rotated. Each
            section (header included) disappears entirely when empty. */}
        {!visuallyCollapsed && pinnedRoutes.length > 0 && (
          <MiniSection labelKey="pinned">
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
        {!visuallyCollapsed && recentRoutes.length > 0 && (
          <MiniSection labelKey="recent">
            {recentRoutes.map((route) => (
              <MiniNavRow
                key={route.id}
                route={route}
                onNavigate={onNavigate}
                action={{ kind: 'pin', onToggle: () => togglePin(route.id) }}
              />
            ))}
          </MiniSection>
        )}

        {PRODUCT_NAV_GROUPS.filter((group) => group !== 'home').map((group) => renderGroup(group))}
      </nav>
    </aside>
  );
}

function MiniSection({ labelKey, children }: { labelKey: string; children: ReactNode }) {
  const { t } = useTranslation('nav');
  return (
    <div className="mb-1.5">
      <div className="font-sidebar-face type-micro px-3.5 pb-1 pt-2.5 font-semibold uppercase tracking-wide text-tx-3">
        {t(labelKey)}
      </div>
      <div className="flex flex-col gap-0.5 px-1.5">{children}</div>
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

/** Row for the Pinned / Recent sections. Shares the fixed-group row metrics
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
      <NavLink
        to={route.path}
        onClick={onNavigate}
        draggable={false}
        className={({ isActive }) =>
          `${cn(
            // Match NavRow metrics (h-sidebar-item / text-xs / text-tx-1) so the
            // Pinned / Recent rows share the fixed groups' density.
            'relative flex h-sidebar-item items-center gap-2 rounded-md pl-2.5 text-xs font-strong text-tx-1',
            // extra right padding for the grip + pin controls (grip only on pinned rows)
            drag ? 'pr-16' : 'pr-9',
            'transition-colors duration-fast ease-default hover:bg-bg-3 hover:text-tx-0',
            'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
            isActive &&
              'bg-indigo-dim text-indigo-soft hover:bg-indigo-dim hover:text-indigo-soft before:absolute before:-left-1.5 before:top-1/2 before:h-5 before:w-0.5 before:-translate-y-1/2 before:rounded-r before:bg-indigo',
          )} font-sidebar-face`
        }
      >
        <route.icon className="h-4 w-4 shrink-0" />
        <span className="flex-1 truncate">{label}</span>
      </NavLink>
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
            className="grid h-7 w-7 cursor-grab place-items-center rounded text-tx-3 opacity-0 transition-opacity hover:bg-bg-2 hover:text-tx-0 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo active:cursor-grabbing group-hover/mini:opacity-100 group-focus-within/mini:opacity-100"
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
            'hover:bg-bg-2 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
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
  const label = t(item.labelKey);
  const link = (
    <NavLink
      to={item.path}
      end={item.exact === true}
      onClick={onNavigate}
      className={({ isActive }) =>
        `${cn(
          // Shell navigation stays stable across density modes; lighter,
          // caption-sized labels and 16px icons keep the compact rail balanced.
          'group relative flex h-sidebar-item items-center gap-2 rounded-md pl-2.5 pr-2 text-xs font-strong text-tx-1',
          'transition-colors duration-fast ease-default',
          'hover:bg-bg-3 hover:text-tx-0',
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
          isActive && 'bg-indigo-dim text-indigo-soft hover:bg-indigo-dim hover:text-indigo-soft',
          collapsed && 'justify-center pl-0 pr-0',
          !collapsed && pinControl && 'pr-10',
        )} font-sidebar-face`
      }
    >
      {({ isActive }) => (
        <>
          {isActive && (
            // Active state: 2px indigo rail flush to the sidebar's left
            // edge. Phase 4 replaces the orange rail (legacy "terminal
            // hacker" accent) with brand indigo so the marker matches
            // focus rings and primary buttons everywhere.
            <span
              aria-hidden
              className="absolute -left-1.5 top-1/2 h-5 w-0.5 -translate-y-1/2 rounded-r bg-indigo"
            />
          )}
          <item.icon className="h-4 w-4 shrink-0" />
          {!collapsed && <span className="flex-1 truncate">{label}</span>}
        </>
      )}
    </NavLink>
  );

  if (collapsed) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>{link}</TooltipTrigger>
        <TooltipContent side="right">{label}</TooltipContent>
      </Tooltip>
    );
  }

  if (!pinControl) return link;

  const actionLabel = pinControl.pinned ? t('unpin') : t('pin');
  return (
    <div className="group/nav relative">
      {link}
      <button
        type="button"
        onClick={pinControl.onToggle}
        aria-label={actionLabel}
        title={actionLabel}
        className={cn(
          'absolute right-1 top-1/2 grid h-7 w-7 -translate-y-1/2 place-items-center rounded text-tx-3 opacity-0 transition-opacity',
          'hover:bg-bg-2 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
          'focus-visible:opacity-100 group-hover/nav:opacity-100 group-focus-within/nav:opacity-100',
          pinControl.pinned && 'text-indigo-soft',
        )}
      >
        <Pin className={cn('h-3 w-3', pinControl.pinned && 'fill-current')} />
      </button>
    </div>
  );
}
