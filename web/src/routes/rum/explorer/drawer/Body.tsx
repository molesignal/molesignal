import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumField, RumRow } from '@/api/rum';
import { useTimeFormatter } from '@/lib/time';
import { JsonView } from '@/shell/JsonEditor';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Button } from '@/shell/ui/button';
import { Skeleton } from '@/shell/ui/skeleton';

import { ViewWaterfall } from '../../waterfall/Waterfall';
import type { RumTab } from '../model';
import { entityOptions, isPresent } from './entity';
import { RUM_SUMMARY, summaryFields } from './facts';
import { RumDrawerFields } from './Fields';
import { RumRowSummary } from './Summary';
import { StackText } from '../stack/StackText';

/** Fields the drawer shows in a place of their own, not in the list of fields. */
const SET_APART = new Set(['error.stack', 'error.frames']);

type Section = 'waterfall' | 'stack' | 'fields' | 'json';

/** The editor's line above and below its text, which its height does not count. */
const EDITOR_EDGES = 2;

/**
 * What the drawer says of a row, under its head: the row is listed with the columns of the table,
 * and read whole in the background. Its message, numbers and where it happened stand first; then
 * its sections: how a view of a browser loaded, an error's stack, every other field, the JSON.
 */
export function RumRowBody({
  tab,
  listed,
  fields,
  span,
  focus,
  focusAt,
}: {
  tab: RumTab;
  listed: RumRow;
  fields: readonly RumField[];
  span?: { from: number; to: number } | undefined;
  focus?: string | undefined;
  focusAt?: number | undefined;
}) {
  const { t, i18n } = useTranslation('rum');
  const language = i18n.resolvedLanguage ?? i18n.language;
  // A moment of the row is written in the time zone the page is read in, as its time in the head is.
  const moment = useTimeFormatter().millis;
  const whole = useQuery(entityOptions(tab, listed, span));
  const row = whole.data ?? listed;
  const byName = React.useMemo(() => new Map(fields.map((field) => [field.name, field])), [fields]);

  // The loading of a page is told for a view of a browser; a screen of an app has no such story. A row
  // that does not say where it was sent from yet (the table lists no such column) is told by its page.
  const loading = tab === 'views' && (row.source === undefined ? isPresent(row['view.url_path']) : row.source === 'browser');
  const stack = typeof row['error.stack'] === 'string' ? (row['error.stack'] as string) : null;
  const sections: Section[] = [...(loading ? (['waterfall'] as const) : []), ...(stack ? (['stack'] as const) : []), 'fields', 'json'];
  // The first section until the reader picks one: a view read whole only then still opens on its load.
  const [chosen, setChosen] = React.useState<Section | null>(null);
  const section = chosen && sections.includes(chosen) ? chosen : sections[0]!;

  const said = summaryFields(RUM_SUMMARY[tab], row);
  const rest = Object.keys(row)
    .filter((name) => name !== 'timestamp' && !SET_APART.has(name) && !said.has(name) && isPresent(row[name]))
    .sort();
  const label: Record<Section, string> = {
    waterfall: t('explorer.drawer.sections.waterfall'),
    stack: t('explorer.drawer.sections.stack'),
    fields: t('explorer.drawer.sections.fields'),
    json: t('explorer.drawer.sections.json'),
  };

  return (
    <>
      <div className="shrink-0 px-4 pb-3">
        {whole.isError && (
          <div data-rum-drawer-failed="" className="mb-2 flex items-center justify-between gap-3 font-sans text-xs text-tx-2">
            <span>{t('explorer.drawer.failed')}</span>
            <Button variant="ghost" size="sm" onClick={() => void whole.refetch()}>
              {t('overview.retry')}
            </Button>
          </div>
        )}
        <RumRowSummary tab={tab} row={row} byName={byName} language={language} moment={moment} />
      </div>
      <div className="shrink-0 border-b border-bd-0 px-4 pb-2">
        <ViewTabs
          label={t('explorer.drawer.sections.aria')}
          value={section}
          onValueChange={setChosen}
          dataAttributes={{ 'data-rum-drawer-sections': '' }}
          items={sections.map((key) => ({ key, label: label[key], ...(key === 'fields' ? { count: rest.length } : {}) }))}
        />
      </div>
      <div data-rum-drawer-body="" className="min-h-0 flex-1 overflow-auto px-4 py-3">
        {(whole.isLoading || (section === 'waterfall' && !whole.data)) && (
          <div role="status" aria-label={t('explorer.drawer.loading')} className="mb-3 flex flex-col gap-[6px]">
            <Skeleton className="h-[14px] w-2/3" />
          </div>
        )}
        {section === 'waterfall' && whole.data && <ViewWaterfall row={whole.data} focus={focus} focusAt={focusAt} />}
        {section === 'stack' && stack && (
          <pre
            data-rum-stack=""
            className="m-0 overflow-auto rounded-md bg-[var(--control-surface)] p-3 font-mono text-xs leading-relaxed text-tx-1"
          >
            <StackText stack={stack} />
          </pre>
        )}
        {section === 'fields' && <RumDrawerFields names={rest} row={row} byName={byName} language={language} moment={moment} />}
        {section === 'json' && <RowJson row={row} label={label.json} />}
      </div>
    </>
  );
}

/**
 * The row as JSON, as tall as the room under the tabs: the box reaches the foot of the drawer
 * however short the row is, and a long row scrolls inside it rather than the drawer.
 */
function RowJson({ row, label }: { row: RumRow; label: string }) {
  const roomRef = React.useRef<HTMLDivElement>(null);
  const [room, setRoom] = React.useState<number | null>(null);

  React.useLayoutEffect(() => {
    const element = roomRef.current;
    if (!element) return;

    const measure = () => setRoom(Math.floor(element.clientHeight));
    const observer = new ResizeObserver(measure);

    measure();
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return (
    <div ref={roomRef} data-rum-drawer-json="" className="h-full min-h-0">
      {room !== null && <JsonView value={row} ariaLabel={label} minHeight={room - EDITOR_EDGES} maxHeight={room - EDITOR_EDGES} />}
    </div>
  );
}
