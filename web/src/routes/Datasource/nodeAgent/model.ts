import type { NodeAgentAssets } from '@/api/nodeAgent';

/** The query key the guide reads the assets under. */
export const NODE_AGENT_ASSETS_KEY = ['node-agent', 'assets'] as const;

/** The four things the Node Agent guide installs, in the order they are listed. */
export type AssetKey = 'indexes' | 'dashboard' | 'views' | 'alerts';

/**
 * How one asset stands: `ready` needs nothing, `pending` is installed by the button, `waiting`
 * comes by itself once the agent's events arrive, `blocked` cannot be installed here.
 */
export type AssetState = 'ready' | 'pending' | 'waiting' | 'blocked';

export interface AssetRow {
  key: AssetKey;
  state: AssetState;
  /** The variant of the row's sentence under `node_agent.assets.<key>.<detail>`. */
  detail: string;
  /** Values the sentence names. */
  values: Record<string, string | number>;
  /** Where the asset is looked at, once it is there. */
  href: string | null;
}

/** Each asset as it stands: what the row says and where it leads. */
export function assetRows(assets: NodeAgentAssets): AssetRow[] {
  const views = assets.views.filter((view) => view.installed).length;
  const alerts = assets.alerts ?? [];
  const made = alerts.filter((alert) => alert.ruleId).length;
  const enabled = alerts.filter((alert) => alert.enabled).length;

  const indexes: AssetRow =
    assets.indexes.state === 'ready'
      ? { key: 'indexes', state: 'ready', detail: 'ready', values: {}, href: null }
      : assets.indexes.state === 'noStream'
        ? { key: 'indexes', state: 'waiting', detail: 'no_stream', values: {}, href: null }
        : {
            key: 'indexes',
            state: 'pending',
            detail: 'missing',
            values: { fields: assets.indexes.fields.join(', ') },
            href: null,
          };

  const dashboardState = assets.dashboard.state;
  const dashboard: AssetRow = {
    key: 'dashboard',
    state: dashboardState === 'current' ? 'ready' : dashboardState === 'taken' ? 'blocked' : 'pending',
    detail: dashboardState,
    values: {},
    href: dashboardState === 'missing' ? null : `/dashboards/${encodeURIComponent(assets.dashboard.id)}`,
  };

  return [
    indexes,
    dashboard,
    {
      key: 'views',
      state: views === assets.views.length ? 'ready' : 'pending',
      detail: 'count',
      values: { installed: views, total: assets.views.length },
      href: views > 0 ? '/saved-views' : null,
    },
    assets.alerts === null
      ? { key: 'alerts', state: 'blocked', detail: 'unreadable', values: {}, href: null }
      : {
          key: 'alerts',
          state: made === alerts.length ? 'ready' : 'pending',
          detail: 'count',
          values: { made, enabled, total: alerts.length },
          href: null,
        },
  ];
}

/** Whether the install button has anything left to do. */
export function hasPending(rows: readonly AssetRow[]): boolean {
  return rows.some((row) => row.state === 'pending');
}

/** Whether the agent's data has arrived: the guide's assets section is done when it has and nothing is pending. */
export function assetsDone(assets: NodeAgentAssets, rows: readonly AssetRow[]): boolean {
  return assets.streams.metrics && assets.streams.events && !hasPending(rows);
}
