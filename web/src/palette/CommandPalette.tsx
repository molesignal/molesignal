import { useQuery } from '@tanstack/react-query';
import { Database, GaugeCircle, LayoutDashboard, type LucideIcon, Network, Sparkles } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';

import * as webApi from '@/api/web';
import {
  canAccessProductPath,
  useProductAccess,
} from '@/product/access';
import { FEATURE_DEFINITIONS, selectFeatureGate, useEditionMetadata, type FeatureKey } from '@/product/edition';
import { useTheme } from '@/shell/ThemeBootstrap';
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from '@/shell/ui/command';
import { Kbd } from '@/shell/ui/kbd';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import { useTimeStore } from '@/stores/useTimeStore';

import { rankResults } from './fuzzy';
import { bumpUsage, buildStaticActions, type OpenMode, type ResultItem } from './registry';

interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}

const ITEM_KIND_ICON: Record<webApi.WebSearchKind, LucideIcon> = {
  stream: Database,
  service: Network,
  dashboard: LayoutDashboard,
  saved_view: Sparkles,
  alert: GaugeCircle,
  incident: GaugeCircle,
};

export function CommandPalette({ open, onOpenChange }: CommandPaletteProps) {
  const { t } = useTranslation(['palette', 'errors', 'nav', 'edition']);
  const [query, setQuery] = React.useState('');
  const [debounced, setDebounced] = React.useState('');
  const nav = useNavigate();
  const location = useLocation();
  const editionMetadata = useEditionMetadata();
  const access = useProductAccess();
  const setWindow = useTimeStore((s) => s.setWindow);
  const togglePin = useTimeStore((s) => s.togglePin);
  const anchor = useTimeStore((s) => s.anchor);
  const stack = useInvestigationStack();
  const logout = useAuthStore((s) => s.logout);
  const { toggleTheme, toggleDensity } = useTheme();

  React.useEffect(() => {
    const t = window.setTimeout(() => setDebounced(query.trim()), 80);
    return () => window.clearTimeout(t);
  }, [query]);

  const remote = useQuery({
    queryKey: ['web', 'search', debounced],
    queryFn: () => webApi.search(debounced),
    enabled: open && debounced.length >= 1,
    staleTime: 10_000,
  });

  const staticActions = React.useMemo<ResultItem[]>(
    () =>
      buildStaticActions({
        setTimeWindow: setWindow,
        toggleTheme,
        toggleDensity,
        pinAnchor: () => togglePin(anchor?.at ?? new Date().toISOString()),
        copyInvestigationLink: () => {
          void navigator.clipboard.writeText(window.location.href).then(() => {
            toast.success(t('errors:link_copied'));
          });
        },
        openHelp: () => {
          // The help overlay listens for `?` globally; we just open it via a custom event.
          window.dispatchEvent(new CustomEvent('molesignal:open-help'));
        },
        signOut: () => {
          logout();
          nav('/signin');
        },
        t: (key) => t(`palette:${key}`),
        tNav: (key) => t(`nav:${key}`),
        tEdition: (key) => t(`edition:${key}`),
        currentPath: location.pathname,
        canAccessPath: (path) => canAccessProductPath(path, access),
        gateStatus: (feature: FeatureKey) => selectFeatureGate(editionMetadata, FEATURE_DEFINITIONS[feature]).status,
      }),
    [setWindow, toggleTheme, toggleDensity, togglePin, anchor, logout, nav, t, location.pathname, editionMetadata, access],
  );

  const remoteItems: ResultItem[] = React.useMemo(() => {
    if (!remote.data?.items) return [];
    return remote.data.items
      .filter((result) =>
        canAccessProductPath(
          {
            stream: '/investigate',
            service: '/investigate',
            dashboard: `/dashboards/${result.id}`,
            saved_view: '/investigate',
            alert: '/alerts/rules',
            incident: '/investigate',
          }[result.kind],
          access,
        ),
      )
      .map((r) => ({
        kind: r.kind,
        id: r.id,
        label: r.label,
        ...(r.subtitle !== undefined && { subtitle: r.subtitle }),
        icon: ITEM_KIND_ICON[r.kind] ?? Database,
      }));
  }, [access, remote.data]);

  const merged = React.useMemo(() => {
    // dedupe by `kind:id`
    const seen = new Set<string>();
    const all: ResultItem[] = [];
    for (const r of [...staticActions, ...remoteItems]) {
      const key = `${r.kind}:${r.id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      all.push(r);
    }
    return rankResults(debounced, all);
  }, [staticActions, remoteItems, debounced]);

  const executeSelect = React.useCallback(
    (item: ResultItem, mode: OpenMode) => {
      bumpUsage(item.id);
      if (item.run) {
        item.run({ mode, navigate: nav });
      } else {
        runRemoteSelection(item, mode, { navigate: nav, stack });
      }
      onOpenChange(false);
      setQuery('');
    },
    [nav, stack, onOpenChange],
  );

  return (
    <CommandDialog open={open} onOpenChange={onOpenChange}>
      <CommandInput
        value={query}
        onValueChange={setQuery}
        placeholder={t('palette:placeholder')}
      />
      <CommandList
        onKeyDownCapture={(e) => {
          // open-mode hotkeys
          if (e.key !== 'Enter') return;
          // The cmdk's onSelect fires already; we route via onSelect below.
        }}
      >
        <CommandEmpty>{t('palette:no_results')}</CommandEmpty>
        {merged.length > 0 && (
          <CommandGroup heading={debounced ? t('palette:groups.results') : t('palette:groups.actions')}>
            {merged.map((item) => (
              <PaletteRow
                key={`${item.kind}:${item.id}`}
                item={item}
                onSelect={(mode) => executeSelect(item, mode)}
              />
            ))}
          </CommandGroup>
        )}
        <CommandSeparator />
      </CommandList>
      <PaletteFooter />
    </CommandDialog>
  );
}

function PaletteRow({ item, onSelect }: { item: ResultItem; onSelect: (mode: OpenMode) => void }) {
  const Icon = item.icon;
  const disabled =
    item.gateStatus !== undefined && item.gateStatus !== 'allowed';
  return (
    <CommandItem
      value={`${item.kind}:${item.id}:${item.label}`}
      disabled={disabled}
      aria-disabled={disabled || undefined}
      onSelect={() => {
        if (!disabled) onSelect('replace');
      }}
      onKeyDown={(e) => {
        if (disabled) return;
        if (e.key === 'Enter') {
          if (e.metaKey || e.ctrlKey) {
            e.preventDefault();
            e.stopPropagation();
            onSelect('new_stack');
          } else if (e.altKey) {
            e.preventDefault();
            e.stopPropagation();
            onSelect('append_layer');
          }
        }
      }}
      // Five-column grid (icon · label/subtitle stack · kind chip · shortcut)
      // gives label predictable space and lets the right-aligned chip + Kbd
      // stay aligned across rows. Selected style is applied on top of the
      // cmdk data-selected attribute via the additional classes below.
      className="!grid !grid-cols-[16px_minmax(0,1fr)_auto_auto] !items-center !gap-3 border-l-2 border-transparent data-[selected=true]:!border-accent data-[selected=true]:!bg-accent-bg/[.12]"
    >
      {Icon ? <Icon className="h-4 w-4 opacity-80" /> : <span className="h-4 w-4" />}
      <div className="flex min-w-0 flex-col">
        <span className="truncate">{item.label}</span>
        {item.subtitle && (
          // Compact density: 1-line ellipsis. Comfortable density: up to
          // two lines via `line-clamp-2`, kicked in by the body density
          // attribute (set by ThemeBootstrap).
          <span className="block truncate text-xs text-muted-foreground [body[data-density=comfortable]_&]:line-clamp-2 [body[data-density=comfortable]_&]:whitespace-normal">
            {item.subtitle}
          </span>
        )}
      </div>
      <span className="shrink-0 rounded-sm border border-border px-1 text-xs uppercase leading-4 text-muted-foreground">
        {item.kind}
      </span>
      {item.shortcut ? <CommandShortcut>{formatShortcut(item.shortcut)}</CommandShortcut> : <span />}
    </CommandItem>
  );
}

function formatShortcut(shortcut: string): string {
  return shortcut
    .split(' ')
    .map((part) =>
      part
        .split('+')
        .map((token) => {
          if (token === 'mod') return '⌘';
          if (token === 'alt') return '⌥';
          if (token === 'shift') return '⇧';
          if (token === 'enter') return 'Enter';
          if (token === 'esc') return 'Esc';
          return token.toUpperCase();
        })
        .join(''),
    )
    .join(' ');
}

function PaletteFooter() {
  const { t } = useTranslation('palette');
  return (
    <div className="flex items-center justify-between border-t border-border px-3 py-1.5 text-xs text-muted-foreground">
      <div className="flex items-center gap-3">
        <span><Kbd size="sm">Enter</Kbd> {t('footer.open')}</span>
        <span><Kbd size="sm">⌘</Kbd><Kbd size="sm">Enter</Kbd> {t('footer.open_new_stack')}</span>
        <span><Kbd size="sm">⌥</Kbd><Kbd size="sm">Enter</Kbd> {t('footer.stack_on_top')}</span>
      </div>
      <span><Kbd size="sm">Esc</Kbd> {t('footer.close')}</span>
    </div>
  );
}

function runRemoteSelection(
  item: ResultItem,
  mode: OpenMode,
  ctx: { navigate: (to: string) => void; stack: ReturnType<typeof useInvestigationStack.getState> },
) {
  // Static actions are dispatched via item.run upstream; this path only handles
  // remote-result kinds.
  if (item.kind === 'action') return;
  const target = {
    stream: { kind: 'log' as const, params: { stream: item.id } },
    service: { kind: 'service' as const, params: { service: item.label } },
    dashboard: null,
    saved_view: { kind: 'saved_view' as const, params: { id: item.id } },
    alert: null,
    incident: { kind: 'incident' as const, params: { id: item.id } },
  }[item.kind];

  if (!target) {
    if (item.kind === 'dashboard') ctx.navigate(`/dashboards/${item.id}`);
    else if (item.kind === 'alert') ctx.navigate(`/alerts/rules`);
    return;
  }

  if (mode === 'new_stack') ctx.stack.reset();
  const parentId =
    mode === 'append_layer' && ctx.stack.frames.length > 0
      ? ctx.stack.frames[ctx.stack.frames.length - 1]!.id
      : undefined;

  ctx.stack.push({ ...target, ...(parentId && { parent_frame_id: parentId }) });
  ctx.navigate('/investigate');
}
