import { describe, expect, it } from 'vitest';

import {
  checkSources,
  dashboardHref,
  formatAliases,
  hasSources,
  isLinkAddress,
  isSelector,
  logSourceHref,
  metricSourceHref,
  parseAliases,
  sourceRows,
  sourcesWrite,
} from './dataSources';

describe('the other names of a service', () => {
  it('reads names typed with commas, Chinese commas or 、 between them', () => {
    expect(parseAliases(' order_service ,订单中心、 orders-legacy，', 'order-api')).toEqual({
      aliases: ['order_service', '订单中心', 'orders-legacy'],
      problem: null,
    });
    expect(parseAliases('', 'order-api')).toEqual({ aliases: [], problem: null });
    expect(formatAliases(['order_service', '订单中心'])).toBe('order_service, 订单中心');
    expect(formatAliases(undefined)).toBe('');
  });

  it('says what the backend would refuse', () => {
    expect(parseAliases('Order-API', 'order-api')).toMatchObject({ problem: 'own_name', at: 'Order-API' });
    expect(parseAliases('orders, ORDERS', 'order-api')).toMatchObject({ problem: 'twice', at: 'ORDERS' });
    expect(parseAliases('x'.repeat(129), 'order-api')).toMatchObject({ problem: 'length' });
    expect(parseAliases(Array.from({ length: 21 }, (_, index) => `name-${index}`).join(','), 'order-api')).toMatchObject({ problem: 'too_many' });
  });
});

describe('where the data of a service is', () => {
  const kept = {
    logs: [{ stream: 'default', filter: "service_name = 'order_service'" }, { stream: 'nginx' }],
    metrics: [{ selector: '{service="order-api"}' }],
    dashboards: ['dash-1'],
    links: [{ name: 'Runbook', url: 'https://wiki.example.com/orders' }],
  };

  it('edits rows and writes them back without the empty ones', () => {
    const rows = sourceRows(kept);

    expect(rows.logs).toEqual([
      { stream: 'default', filter: "service_name = 'order_service'" },
      { stream: 'nginx', filter: '' },
    ]);

    rows.metrics.push('  ');
    rows.links.push({ name: ' ', url: '' });
    rows.logs[1]!.stream = ' nginx ';

    expect(sourcesWrite(rows)).toEqual({
      logs: [
        { stream: 'default', filter: "service_name = 'order_service'" },
        { stream: 'nginx', filter: null },
      ],
      metrics: [{ selector: '{service="order-api"}' }],
      dashboards: ['dash-1'],
      links: [{ name: 'Runbook', url: 'https://wiki.example.com/orders' }],
    });
    expect(checkSources(rows)).toBeNull();
    expect(hasSources(kept)).toBe(true);
    expect(hasSources({})).toBe(false);
    expect(hasSources(undefined)).toBe(false);
  });

  it('says which row the backend would refuse', () => {
    const rows = sourceRows(kept);

    rows.logs.push({ stream: '', filter: "host = 'a'" });
    expect(checkSources(rows)).toEqual({ kind: 'logs', index: 2, problem: 'stream' });
    rows.logs.pop();

    rows.metrics[0] = '{service="order-api"';
    expect(checkSources(rows)).toEqual({ kind: 'metrics', index: 0, problem: 'selector' });
    rows.metrics[0] = 'http_requests_total{app="orders"}';

    rows.links[0]!.url = 'wiki.example.com/orders';
    expect(checkSources(rows)).toEqual({ kind: 'links', index: 0, problem: 'link_url' });
    rows.links[0]!.url = 'https://wiki.example.com/orders';

    rows.dashboards.push('dash-1');
    expect(checkSources(rows)).toEqual({ kind: 'dashboards', index: 1, problem: 'dashboard_twice' });
    rows.dashboards.pop();

    rows.metrics = Array.from({ length: 11 }, (_, index) => `metric_${index}`);
    expect(checkSources(rows)).toEqual({ kind: 'metrics', problem: 'too_many' });
  });

  it('takes selectors and link addresses as the backend does', () => {
    for (const selector of ['{service="order-api"}', 'orders_queue_depth', 'http_requests_total{app="orders"}']) {
      expect(isSelector(selector), selector).toBe(true);
    }

    for (const selector of ['', '{a="b"', 'a{b="c"}{d="e"}', '{a="b"} + 1', 'a\nb']) {
      expect(isSelector(selector), selector).toBe(false);
    }

    expect(isLinkAddress('https://wiki.example.com/orders')).toBe(true);
    expect(isLinkAddress('http://10.0.0.1:8080/x')).toBe(true);
    expect(isLinkAddress('ftp://example.com')).toBe(false);
    expect(isLinkAddress('https://')).toBe(false);
    expect(isLinkAddress('https://exa mple.com')).toBe(false);
  });

  it('opens a source where its data is read', () => {
    const logs = new URL(logSourceHref({ stream: 'default', filter: "service_name = 'order_service'" }), 'http://x');

    expect(logs.pathname).toBe('/logs');
    expect(logs.searchParams.get('stream')).toBe('default');
    expect(logs.searchParams.get('sql')).toBe(`SELECT * FROM "default"\nWHERE service_name = 'order_service'\nORDER BY _timestamp DESC\nLIMIT 200`);
    expect(new URL(logSourceHref({ stream: 'nginx' }), 'http://x').searchParams.get('sql')).toBeNull();
    expect(new URL(metricSourceHref({ selector: '{service="order-api"}' }), 'http://x').searchParams.get('promql')).toBe('{service="order-api"}');
    expect(dashboardHref('dash 1')).toBe('/dashboards/dash%201');
  });
});
