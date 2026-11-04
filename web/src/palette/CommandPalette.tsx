import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, ChevronDown, Database, GaugeCircle, LayoutDashboard, type LucideIcon, Network, Sparkles } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';

import * as incidentsApi from '@/api/incidents';
import * as webApi from '@/api/web';
import { writeClipboardText } from '@/lib/clipboard';
import { toApiError } from '@/lib/http';
import {
  canAccessProductPath,
  useProductAccess,
} from '@/product/access';
import { ChromeButton } from '@/shell/chrome';
import { courseDuration } from '@/shell/incident/Course';
import { refreshIncidents } from '@/shell/incident/refresh';
import { SEVERITY_TONE } from '@/shell/incident/tones';
import { byUrgency, isWaitingOn, rowsOf } from '@/shell/notifications/model';
import { StatusWord, type StatusTone } from '@/shell/StatusWord';
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
import { signOut } from '@/stores/auth/signOut';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import { useTimeStore } from '@/stores/useTimeStore';

import { rankResults } from './fuzzy';
import { groupResults } from './groups';
import { readRecents, rememberRecent } from './recent';
import { bumpUsage, buildStaticActions, type ItemKind, type OpenMode, type ResultItem } from './registry';

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
  const { t } = useTranslation(['palette', 'errors', 'nav']);
  const [query, setQuery] = React.useState('');
  const [debounced, setDebounced] = React.useState('');
  const nav = useNavigate();
  const location = useLocation();
  const access = useProductAccess();
  const setWindow = useTimeStore((s) => s.setWindow);
  const togglePin = useTimeStore((s) => s.togglePin);
  const anchor = useTimeStore((s) => s.anchor);
  const stack = useInvestigationStack();
  const { toggleTheme, toggleDensity } = useTheme();
  const queryClient = useQueryClient();
  const currentUserId = useAuthStore((state) => state.ctx?.userId ?? '');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const [expanded, setExpanded] = React.useState<ReadonlySet<ItemKind>>(() => new Set());

  React.useEffect(() => {
    const t = window.setTimeout(() => setDebounced(query.trim()), 80);
    return () => window.clearTimeout(t);
  }, [query]);

  // A new search starts with each kind's first few again.
  React.useEffect(() => setExpanded(new Set()), [debounced]);

  const incidents = useQuery({
    queryKey: ['alerts', 'incidents', 'active'],
    queryFn: () => incidentsApi.list(),
    enabled: open && canAccessProductPath('/alerts/incidents', access),
    refetchInterval: 30_000,
  });

  const acknowledge = useMutation({
    mutationFn: (id: string) => incidentsApi.ack(id),
    onSuccess: () => {
      toast.success(t('palette:acknowledged'));
      void refreshIncidents(queryClient);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

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
          void writeClipboardText(window.location.href).then(() => {
            toast.success(t('errors:link_copied'));
          });
        },
        openHelp: () => {
          // The help overlay listens for `?` globally; we just open it via a custom event.
          window.dispatchEvent(new CustomEvent('molesignal:open-help'));
        },
        signOut: () => {
          void signOut().then(() => nav('/signin'));
        },
        t: (key) => t(`palette:${key}`),
        tNav: (key) => t(`nav:${key}`),
        currentPath: location.pathname,
        canAccessPath: (path) => canAccessProductPath(path, access),
      }),
    [setWindow, toggleTheme, toggleDensity, togglePin, anchor, nav, t, location.pathname, access],
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

  const nowMicros = Date.now() * 1000;
  const attention: ResultItem[] = rowsOf(incidents.data)
    .filter((incident) => isWaitingOn(incident, currentUserId))
    .sort(byUrgency)
    .slice(0, 5)
    .map((incident) => ({
      kind: 'incident',
      id: incident.id,
      label: incident.summary || incident.id,
      subtitle: [incident.affectedServices[0], courseDuration(nowMicros - incident.createdAt)]
        .filter(Boolean)
        .join(' · '),
      icon: GaugeCircle,
      severity: incident.severity,
      acknowledgeId: incident.id,
      run: ({ navigate }) => navigate(`/alerts/incidents/${encodeURIComponent(incident.id)}`),
    }));

  // Asked once per opening: what is chosen now shows up the next time.
  const recentEntries = React.useMemo(() => (open ? readRecents(orgId) : []), [open, orgId]);
  const recents: ResultItem[] = recentEntries.flatMap((entry) => {
    if (entry.kind !== 'action') return [{ ...entry, icon: ITEM_KIND_ICON[entry.kind] ?? Database }];
    // A place that is no longer offered here (a role changed) is no longer offered.
    const action = staticActions.find((item) => item.id === entry.id);
    return action ? [action] : [];
  });
  const recentKeys = new Set(recents.map((item) => `${item.kind}:${item.id}`));

  const sections = debounced
    ? groupResults(merged, expanded).map((group) => ({
        key: group.kind,
        heading: t(`palette:groups.kinds.${group.kind}`),
        items: group.items,
        hidden: group.hidden,
      }))
    : [
        { key: 'attention', heading: t('palette:groups.attention'), items: attention, hidden: 0 },
        { key: 'recent', heading: t('palette:groups.recent'), items: recents, hidden: 0 },
        {
          key: 'actions',
          heading: t('palette:groups.actions'),
          items: merged.filter((item) => !recentKeys.has(`${item.kind}:${item.id}`)),
          hidden: 0,
        },
      ].filter((section) => section.items.length > 0);
  const offersAcknowledge = sections.some((section) => section.items.some((item) => item.acknowledgeId));

  const executeSelect = React.useCallback(
    (item: ResultItem, mode: OpenMode) => {
      bumpUsage(item.id);
      if (!item.acknowledgeId) rememberRecent(item, orgId);
      if (item.run) {
        item.run({ mode, navigate: nav });
      } else {
        runRemoteSelection(item, mode, { navigate: nav, stack });
      }
      onOpenChange(false);
      setQuery('');
    },
    [nav, stack, onOpenChange, orgId],
  );

  return (
    <CommandDialog open={open} onOpenChange={onOpenChange} shouldFilter={false}>
      <CommandInput
        value={query}
        onValueChange={setQuery}
        placeholder={t('palette:placeholder')}
        onKeyDown={(event) => {
          // The focus stays in the field: the row in question is the one the arrows are on.
          if (!event.altKey || event.code !== 'KeyA') return;
          const row = document.querySelector('[cmdk-item][data-selected="true"]');
          const id = row?.getAttribute('data-acknowledge');
          if (!id) return;
          event.preventDefault();
          acknowledge.mutate(id);
        }}
      />
      <CommandList>
        <CommandEmpty>{t('palette:no_results')}</CommandEmpty>
        {sections.map((section) => (
          <CommandGroup key={section.key} heading={section.heading}>
            {section.items.map((item) => (
              <PaletteRow
                key={`${section.key}:${item.kind}:${item.id}`}
                item={item}
                onSelect={(mode) => executeSelect(item, mode)}
                onAcknowledge={item.acknowledgeId ? () => acknowledge.mutate(item.id) : undefined}
                acknowledging={acknowledge.isPending}
              />
            ))}
            {section.hidden > 0 && (
              <CommandItem
                value={`more:${section.key}`}
                onSelect={() => setExpanded((current) => new Set(current).add(section.key as ItemKind))}
                className="text-xs text-tx-2"
              >
                <ChevronDown className="h-4 w-4 opacity-80" />
                {t('palette:show_all', { count: section.hidden })}
              </CommandItem>
            )}
          </CommandGroup>
        ))}
        <CommandSeparator />
      </CommandList>
      <PaletteFooter offersAcknowledge={offersAcknowledge} />
    </CommandDialog>
  );
}

function PaletteRow({
  item,
  onSelect,
  onAcknowledge,
  acknowledging,
}: {
  item: ResultItem;
  onSelect: (mode: OpenMode) => void;
  onAcknowledge?: (() => void) | undefined;
  acknowledging: boolean;
}) {
  const { t } = useTranslation(['palette', 'alerts']);
  const Icon = item.icon;
  return (
    <CommandItem
      value={`${item.kind}:${item.id}:${item.label}`}
      data-acknowledge={item.acknowledgeId}
      onSelect={() => {
        onSelect('replace');
      }}
      onKeyDown={(e) => {
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
      // Three columns (icon · label/subtitle stack · what is on the right) give the
      // label predictable space and keep the right side aligned across rows. What a
      // row is, its group says. Selected style is applied on top of the cmdk
      // data-selected attribute via the additional classes below.
      className="!grid !grid-cols-[16px_minmax(0,1fr)_auto] !items-center !gap-3 border-l-2 border-transparent data-[selected=true]:!border-accent data-[selected=true]:!bg-accent-bg/[.12]"
    >
      {Icon ? <Icon className="h-4 w-4 opacity-80" /> : <span className="h-4 w-4" />}
      <div className="flex min-w-0 flex-col">
        <span className="truncate">{item.label}</span>
        {(item.subtitle || item.severity) && (
          // Compact density: 1-line ellipsis. Comfortable density: up to
          // two lines via `line-clamp-2`, kicked in by the body density
          // attribute (set by ThemeBootstrap).
          <span className="block truncate text-xs text-muted-foreground [body[data-density=comfortable]_&]:line-clamp-2 [body[data-density=comfortable]_&]:whitespace-normal">
            {item.severity && (
              <StatusWord tone={SEVERITY_TONE[item.severity] as StatusTone} className="mr-1.5">
                {t(`alerts:history.severity.${item.severity}`)}
              </StatusWord>
            )}
            {item.subtitle}
          </span>
        )}
      </div>
      {onAcknowledge ? (
        <ChromeButton
          size="sm"
          className="h-7 px-2"
          disabled={acknowledging}
          onClick={(event) => {
            event.stopPropagation();
            onAcknowledge();
          }}
        >
          <Check className="h-3 w-3" />
          {t('palette:acknowledge')}
        </ChromeButton>
      ) : item.shortcut ? (
        <CommandShortcut>{formatShortcut(item.shortcut)}</CommandShortcut>
      ) : (
        <span />
      )}
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

function PaletteFooter({ offersAcknowledge }: { offersAcknowledge: boolean }) {
  const { t } = useTranslation('palette');
  return (
    <div className="flex items-center justify-between border-t border-border px-3 py-1.5 text-xs text-muted-foreground">
      <div className="flex items-center gap-3">
        <span><Kbd size="sm">Enter</Kbd> {t('footer.open')}</span>
        <span><Kbd size="sm">⌘</Kbd><Kbd size="sm">Enter</Kbd> {t('footer.open_new_stack')}</span>
        <span><Kbd size="sm">⌥</Kbd><Kbd size="sm">Enter</Kbd> {t('footer.stack_on_top')}</span>
        {offersAcknowledge && (
          <span><Kbd size="sm">⌥</Kbd><Kbd size="sm">A</Kbd> {t('footer.acknowledge')}</span>
        )}
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

  ctx.stack.push({ ...target, ...(parentId && { parentFrameId: parentId }) });
  ctx.navigate('/investigate');
}
