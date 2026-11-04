import { ArrowLeft, ChevronRight } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import { TraceOperationName } from '@/viz/trace/TraceOperationName';

/** What a drawer under another is named by in its trail, once it knows. */
interface TrailLabels {
  error?: string;
}

interface TrailContextValue {
  labels: TrailLabels;
  setLabel: (layer: keyof TrailLabels, label: string | null) => void;
}

const TrailContext = React.createContext<TrailContextValue | null>(null);

export function DrawerTrailProvider({ children }: { children: React.ReactNode }) {
  const [labels, setLabels] = React.useState<TrailLabels>({});
  const setLabel = React.useCallback((layer: keyof TrailLabels, label: string | null) => {
    setLabels((current) => {
      if ((current[layer] ?? null) === label) return current;
      const next = { ...current };
      if (label === null) delete next[layer];
      else next[layer] = label;
      return next;
    });
  }, []);
  const value = React.useMemo(() => ({ labels, setLabel }), [labels, setLabel]);

  return <TrailContext.Provider value={value}>{children}</TrailContext.Provider>;
}

/** Names an open drawer in the trail of the drawers opened over it. */
export function useTrailLabel(layer: keyof TrailLabels, label: string | null | undefined) {
  const setLabel = React.useContext(TrailContext)?.setLabel;

  React.useEffect(() => {
    if (!setLabel) return undefined;
    setLabel(layer, label ?? null);
    return () => setLabel(layer, null);
  }, [label, layer, setLabel]);
}

/** The drawers a page keeps in its address, by their keys there. */
export type DrawerLayer = 'endpoint' | 'error';

export const DRAWER_LAYERS: readonly DrawerLayer[] = ['endpoint', 'error'];

/** What else a drawer keeps in the address, gone with it. */
const COMPANIONS: Record<DrawerLayer, readonly string[]> = { endpoint: ['kind'], error: [] };

/** The drawers open on the page, the first opened first: the order their keys stand in the address. */
export function drawerLayers(searchParams: URLSearchParams): DrawerLayer[] {
  const layers: DrawerLayer[] = [];
  for (const key of searchParams.keys()) {
    const layer = DRAWER_LAYERS.find((candidate) => candidate === key);
    if (layer && !layers.includes(layer) && searchParams.get(layer)) layers.push(layer);
  }
  return layers;
}

interface Crumb {
  key: DrawerLayer;
  name: string;
  label: React.ReactNode;
  back: () => void;
}

/**
 * The drawers open under this one, the first opened first, each a way back
 * to it: an endpoint and the error opened from it, or an error and the
 * endpoint opened from it, then a trace. A click on one closes every drawer
 * over it. The drawer the trail stands in is named by its own title, so the
 * trail stops short of it, and with nothing under the drawer there is none.
 */
export function DrawerTrail({ over, onLeave }: { over: DrawerLayer | 'trace'; onLeave?: () => void }) {
  const { t } = useTranslation('apm');
  const [searchParams, setSearchParams] = useSearchParams();
  const labels = React.useContext(TrailContext)?.labels ?? {};
  const layers = drawerLayers(searchParams);
  const under = over === 'trace' ? layers : layers.slice(0, Math.max(0, layers.indexOf(over)));

  const crumbs: Crumb[] = under.map((layer, index) => {
    const value = searchParams.get(layer) ?? '';
    const name = layer === 'endpoint' ? value : labels.error ?? value.slice(0, 12);
    return {
      key: layer,
      name,
      label:
        layer === 'endpoint' ? (
          <TraceOperationName operation={value} className="min-w-0" />
        ) : (
          <span className="truncate font-strong text-[var(--red-ink)]">{name}</span>
        ),
      back: () => {
        onLeave?.();
        const above = layers.slice(index + 1);
        if (above.length === 0) return;
        setSearchParams((current) => {
          const params = new URLSearchParams(current);
          for (const closed of above) {
            params.delete(closed);
            for (const companion of COMPANIONS[closed]) params.delete(companion);
          }
          return params;
        });
      },
    };
  });

  if (crumbs.length === 0) return null;

  return (
    <nav aria-label={t('drawer.trail')} data-apm-drawer-trail={over} className="flex min-w-0 items-center gap-1 font-sans text-xs text-tx-2">
      <ArrowLeft aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
      {crumbs.map((crumb, index) => (
        <React.Fragment key={crumb.key}>
          {index > 0 && <ChevronRight aria-hidden className="h-3 w-3 shrink-0 text-tx-4" />}
          <button
            type="button"
            data-apm-drawer-crumb={crumb.key}
            onClick={crumb.back}
            aria-label={t('drawer.back_to', { name: crumb.name })}
            title={t('drawer.back_to', { name: crumb.name })}
            className="inline-flex min-w-0 max-w-[50%] items-baseline rounded-sm text-left text-tx-1 hover:text-tx-0 hover:underline focus-visible:text-tx-0 focus-visible:underline focus-visible:outline-none"
          >
            {crumb.label}
          </button>
        </React.Fragment>
      ))}
    </nav>
  );
}
