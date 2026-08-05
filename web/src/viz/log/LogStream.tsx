import { useVirtualizer } from '@tanstack/react-virtual';
import { ArrowDown } from 'lucide-react';
import * as React from 'react';

import { Button } from '@/shell/ui/button';
import { Input } from '@/shell/ui/input';
import { useThemePalette } from '@/viz/timeseries/themeAdapter';

import { HoverPreview } from './HoverPreview';
import { colorKeyForLevel, colorKeyForService } from './levelColor';
import type { LogRow } from './types';

interface LogStreamProps {
  rows: LogRow[];
  isLive?: boolean;
  onToggleLive?: () => void;
  onRowOpen?: (row: LogRow) => void;
  className?: string;
}

const ROW_H_COMPACT = 24;
const ROW_H_COMFORTABLE = 32;
const HOVER_DELAY_MS = 300;

export function LogStream({ rows, isLive, onToggleLive, onRowOpen, className }: LogStreamProps) {
  const parentRef = React.useRef<HTMLDivElement | null>(null);
  const { palette } = useThemePalette();
  const [selectedIdx, setSelectedIdx] = React.useState(0);
  const [filter, setFilter] = React.useState('');
  const [filterOpen, setFilterOpen] = React.useState(false);
  const [hoverState, setHoverState] = React.useState<{ row: LogRow | null; top: number }>({ row: null, top: 0 });
  const hoverTimerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  const filtered = React.useMemo(() => {
    if (!filter.trim()) return rows;
    const q = filter.toLowerCase();
    return rows.filter((r) =>
      Object.values(r).some((v) => typeof v === 'string' && v.toLowerCase().includes(q)),
    );
  }, [rows, filter]);

  const density = (typeof document !== 'undefined' && document.body.getAttribute('data-density')) || 'compact';
  const rowH = density === 'comfortable' ? ROW_H_COMFORTABLE : ROW_H_COMPACT;

  const virtualizer = useVirtualizer({
    count: filtered.length,
    estimateSize: () => rowH,
    overscan: 10,
    getScrollElement: () => parentRef.current,
  });

  // Auto-stick to bottom while tailing.
  React.useEffect(() => {
    if (!isLive) return;
    const el = parentRef.current;
    if (!el) return;
    const distFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distFromBottom < 40) {
      el.scrollTop = el.scrollHeight;
    }
  }, [filtered.length, isLive]);

  // Keyboard within the log stream
  React.useEffect(() => {
    const el = parentRef.current;
    if (!el) return;
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.tagName === 'INPUT') return;
      if (!e.metaKey && !e.ctrlKey) return;
      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault();
          setSelectedIdx((i) => Math.min(filtered.length - 1, i + (e.shiftKey ? 10 : 1)));
          break;
        case 'ArrowUp':
          e.preventDefault();
          setSelectedIdx((i) => Math.max(0, i - (e.shiftKey ? 10 : 1)));
          break;
        case 'Home':
          e.preventDefault();
          setSelectedIdx(0);
          break;
        case 'End':
          e.preventDefault();
          setSelectedIdx(filtered.length - 1);
          break;
        case 'Enter': {
          e.preventDefault();
          const r = filtered[selectedIdx];
          if (r && onRowOpen) onRowOpen(r);
          break;
        }
        case '/':
          e.preventDefault();
          setFilterOpen(true);
          break;
        case 'Escape':
          if (filterOpen) {
            setFilterOpen(false);
            setFilter('');
          }
          break;
        default:
          break;
      }
    };
    el.addEventListener('keydown', onKey);
    return () => el.removeEventListener('keydown', onKey);
  }, [filtered, selectedIdx, onRowOpen, filterOpen]);

  // Scroll selected into view
  React.useEffect(() => {
    virtualizer.scrollToIndex(selectedIdx, { align: 'auto' });
  }, [selectedIdx, virtualizer]);

  // "New rows" badge if scrolled up during live tail
  const [scrollAway, setScrollAway] = React.useState(false);
  const lastSeenCount = React.useRef(0);
  React.useEffect(() => {
    const el = parentRef.current;
    if (!el) return;
    const onScroll = () => {
      const dist = el.scrollHeight - el.scrollTop - el.clientHeight;
      setScrollAway(dist > 40);
    };
    el.addEventListener('scroll', onScroll);
    return () => el.removeEventListener('scroll', onScroll);
  }, []);
  React.useEffect(() => {
    if (!scrollAway) lastSeenCount.current = filtered.length;
  }, [filtered.length, scrollAway]);
  const newRowsCount = scrollAway ? filtered.length - lastSeenCount.current : 0;

  return (
    <div className={className} style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <header className="flex items-center gap-2 border-b border-border px-3 py-1.5 text-xs">
        <span className="text-muted-foreground">{filtered.length} rows</span>
        {onToggleLive && (
          <Button variant={isLive ? 'default' : 'outline'} size="sm" onClick={onToggleLive}>
            {isLive ? 'Live ●' : 'Live'}
          </Button>
        )}
        {filterOpen && (
          <Input
            autoFocus
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="filter substring (Esc to clear)"
            className="h-7 text-xs"
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                setFilter('');
                setFilterOpen(false);
              }
            }}
          />
        )}
      </header>
      <div
        ref={parentRef}
        tabIndex={0}
        className="relative flex-1 overflow-auto outline-none"
        aria-label="Log stream"
      >
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {virtualizer.getVirtualItems().map((vRow) => {
            const row = filtered[vRow.index]!;
            const isSelected = vRow.index === selectedIdx;
            const levelKey = colorKeyForLevel(row.level);
            const levelBg = levelKey ? palette[`${levelKey.slice(0, -2)}-bg` as keyof typeof palette] : 'transparent';
            const levelFg = levelKey ? palette[levelKey] : palette['--fg-muted'];
            const serviceKey = row.service ? colorKeyForService(row.service) : '--fg-muted';
            return (
              <div
                key={vRow.key}
                data-row=""
                role="row"
                aria-selected={isSelected}
                style={{
                  position: 'absolute',
                  top: vRow.start,
                  height: vRow.size,
                  width: '100%',
                }}
                className={
                  'flex items-center gap-2 border-l-2 px-3 font-sans text-xs ' +
                  (isSelected ? 'border-accent bg-accent-bg' : 'border-transparent hover:bg-muted')
                }
                onMouseEnter={() => {
                  if (hoverTimerRef.current) globalThis.clearTimeout(hoverTimerRef.current);
                  hoverTimerRef.current = globalThis.setTimeout(() => {
                    setHoverState({ row, top: vRow.start });
                  }, HOVER_DELAY_MS);
                }}
                onMouseLeave={() => {
                  if (hoverTimerRef.current) globalThis.clearTimeout(hoverTimerRef.current);
                  hoverTimerRef.current = null;
                  setHoverState({ row: null, top: 0 });
                }}
                onClick={() => setSelectedIdx(vRow.index)}
                onDoubleClick={() => onRowOpen?.(row)}
              >
                <span className="w-[80px] shrink-0 text-muted-foreground">{tsLabel(row._timestamp)}</span>
                <span
                  className="inline-flex h-4 shrink-0 items-center rounded-sm px-1.5 text-xs uppercase"
                  style={{ background: levelBg, color: levelFg }}
                >
                  {(row.level ?? 'info').slice(0, 4)}
                </span>
                {row.service && (
                  <span className="w-[140px] shrink-0 truncate" style={{ color: palette[serviceKey] }}>
                    {row.service}
                  </span>
                )}
                <span className="min-w-0 flex-1 truncate text-foreground">{row.message ?? ''}</span>
              </div>
            );
          })}
        </div>
        <HoverPreview row={hoverState.row} top={hoverState.top} />
        {scrollAway && newRowsCount > 0 && (
          <button
            type="button"
            className="absolute bottom-3 right-3 z-20 flex items-center gap-1 rounded-md border border-border bg-surface px-2 py-1 text-xs shadow-md hover:bg-muted"
            onClick={() => {
              const el = parentRef.current;
              if (el) el.scrollTop = el.scrollHeight;
            }}
          >
            <ArrowDown className="h-3 w-3" />
            {newRowsCount} new rows
          </button>
        )}
      </div>
    </div>
  );
}

function tsLabel(microseconds: number): string {
  const d = new Date(microseconds / 1000);
  return d.toISOString().slice(11, 23);
}
