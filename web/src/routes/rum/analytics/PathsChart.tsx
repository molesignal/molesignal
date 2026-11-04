import { useTranslation } from 'react-i18next';

import { type Paths, PATHS_OTHER } from '@/api/rum';

import { layoutSankey, type SankeyNode, usedColumns } from './sankey';
import { formatCount } from '../overview/format';

const BOX = { width: 960, height: 340, nodeWidth: 12, gap: 8, margin: 170, maxPerSession: 28 } as const;
const LABEL_CHARS = 22;
/** The height one name takes, and so how near two names can be. */
const LINE = 16;

const cut = (text: string): string => (text.length > LABEL_CHARS ? `${text.slice(0, LABEL_CHARS - 1)}…` : text);

/** Where the name of a page is drawn: the middle of the page, or near its top when it is tall. */
const labelY = (node: SankeyNode): number => node.y + Math.min(node.height / 2, LINE / 2);

/**
 * The pages whose names are drawn. A column's names go from the top down and one that would touch
 * the name above it is left out; it is still said by the tooltip and the label of the page.
 */
function named(nodes: readonly SankeyNode[]): Set<string> {
  const shown = new Set<string>();
  const columns = new Map<number, SankeyNode[]>();

  for (const node of nodes) columns.set(node.column, [...(columns.get(node.column) ?? []), node]);

  for (const column of columns.values()) {
    let bottom = Number.NEGATIVE_INFINITY;

    for (const node of [...column].sort((a, b) => a.y - b.y)) {
      const top = labelY(node) - LINE / 2;

      if (top >= bottom) {
        shown.add(node.key);
        bottom = labelY(node) + LINE / 2;
      }
    }
  }

  return shown;
}

/**
 * The paths as columns of pages: each page is as tall as the sessions that were on it, and a ribbon
 * between two pages is as thick as the sessions that went that way. A page is a button: it becomes
 * the page the paths start from.
 */
export function PathsChart({
  paths,
  anchor,
  onPick,
  language,
}: {
  paths: Paths;
  /** The page the paths start from, which is not picked again. */
  anchor: string;
  onPick: (page: string) => void;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const used = usedColumns(paths.nodes, paths.links);
  const layout = layoutSankey(used.nodes, used.links, used.columns, BOX);
  const shown = named(layout.nodes);
  const name = (page: string) => (page === PATHS_OTHER ? t('analytics.paths.other') : page);
  const endings = (node: SankeyNode) =>
    node.ends > 0 ? ` · ${t(paths.backward ? 'analytics.paths.begins' : 'analytics.paths.ends', { count: node.ends })}` : '';

  const label = (node: SankeyNode) => {
    // The first column's names stand in the room left of it, the others' to the right of their page.
    const before = node.column === 0;
    const picked = node.page !== PATHS_OTHER && node.page !== anchor;
    const full = `${name(node.page)} · ${formatCount(node.sessions, language)}`;

    return (
      <g
        key={node.key}
        data-rum-path-node={`${node.column}:${node.page}`}
        data-rum-path-sessions={node.sessions}
        role={picked ? 'button' : undefined}
        tabIndex={picked ? 0 : undefined}
        aria-label={picked ? t('analytics.paths.pick', { page: name(node.page) }) : undefined}
        onClick={picked ? () => onPick(node.page) : undefined}
        onKeyDown={
          picked
            ? (event) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault();
                  onPick(node.page);
                }
              }
            : undefined
        }
        className={
          picked
            ? 'cursor-pointer outline-none [&:focus-visible>rect]:stroke-[var(--indigo)] [&:focus-visible>rect]:stroke-2 [&:hover>text]:fill-[var(--tx-0)]'
            : undefined
        }
      >
        <title>{`${full}${endings(node)}`}</title>
        <rect
          x={node.x}
          y={node.y}
          width={node.width}
          height={node.height}
          rx={2}
          style={{ fill: node.page === PATHS_OTHER ? 'var(--tx-4)' : 'var(--chart-1)' }}
        />
        {shown.has(node.key) && (
          <text
            x={before ? node.x - 6 : node.x + node.width + 6}
            y={labelY(node) + 4}
            textAnchor={before ? 'end' : 'start'}
            className="fill-[var(--tx-1)] font-sans text-xs"
            style={{ paintOrder: 'stroke', stroke: 'var(--functional-surface)', strokeWidth: 3, strokeLinejoin: 'round' }}
          >
            {`${cut(name(node.page))} · ${formatCount(node.sessions, language)}`}
          </text>
        )}
      </g>
    );
  };

  return (
    <div className="overflow-x-auto px-3 py-3">
      <svg
        data-rum-paths-chart=""
        role="group"
        aria-label={t('analytics.paths.chart_aria', { sessions: formatCount(paths.sessions, language) })}
        viewBox={`0 0 ${BOX.width} ${Math.max(60, layout.height + 8)}`}
        className="h-auto w-full min-w-[640px]"
      >
        <g aria-hidden>
          {layout.links.map((link) => (
            <path
              key={link.key}
              data-rum-path-link={`${link.source.column}:${link.source.page}>${link.target.page}`}
              data-rum-path-sessions={link.sessions}
              d={link.d}
              fill="none"
              strokeWidth={link.thickness}
              className="opacity-30 transition-opacity hover:opacity-60"
              style={{ stroke: 'var(--chart-1)' }}
            >
              <title>{`${name(link.source.page)} → ${name(link.target.page)} · ${formatCount(link.sessions, language)}`}</title>
            </path>
          ))}
        </g>
        {layout.nodes.map(label)}
      </svg>
    </div>
  );
}
