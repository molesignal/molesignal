import { describe, expect, it } from 'vitest';

import { chartsOf, familiesFor, RUNTIME_FAMILIES } from './families';
import { instanceOf, runtimeStatement, seriesOfResult } from './metrics';

const family = (key: string) => RUNTIME_FAMILIES.find((entry) => entry.key === key)!;

describe('runtime metrics of a service', () => {
  it('quotes the dotted labels and the values it matches on, an instance too when one is chosen', () => {
    const { candidate } = chartsOf(family('process'), new Set(['process_cpu_seconds_total']))[0]!;
    expect(runtimeStatement(candidate, { name: 'BY"check', namespace: 'shop' }, { label: 'k8s.pod.name', value: 'pod-1' })).toBe(
      'sum by ("service.instance.id","k8s.pod.name","host.name") (rate(process_cpu_seconds_total{"service.name"="BY\\"check","service.namespace"="shop","k8s.pod.name"="pod-1"}[5m]))',
    );
  });

  it('reads a heap under either convention, and a mean as a sum over a count', () => {
    const heap = chartsOf(family('jvm'), new Set(['jvm_memory_used_bytes'])).find(({ spec }) => spec.chart === 'heap')!;
    expect(runtimeStatement(heap.candidate, { name: 'api' })).toBe(
      '(sum by ("service.instance.id","k8s.pod.name","host.name") (jvm_memory_used_bytes{"service.name"="api","jvm.memory.type"="heap"})) or (sum by ("service.instance.id","k8s.pod.name","host.name") (jvm_memory_used_bytes{"service.name"="api",area="heap"}))',
    );

    const scheduler = chartsOf(family('go'), new Set(['go_sched_latencies_seconds_sum'])).find(({ spec }) => spec.chart === 'scheduler')!;
    expect(runtimeStatement(scheduler.candidate, { name: 'api' })).toContain('rate(go_sched_latencies_seconds_count{"service.name"="api"}[5m])');
  });

  it('takes the process and the runtimes of the languages the service reports, else what the catalog holds', () => {
    expect(familiesFor(['java'], new Set()).map((entry) => entry.key)).toEqual(['process', 'jvm']);
    expect(familiesFor(['erlang'], new Set()).map((entry) => entry.key)).toEqual(['process', 'beam']);
    expect(familiesFor([], new Set(['go_goroutines', 'phpfpm_active_processes'])).map((entry) => entry.key)).toEqual(['process', 'go', 'php']);
  });

  it('draws each chart from the first name the catalog holds', () => {
    const charts = chartsOf(family('go'), new Set(['go_goroutines', 'go_memstats_heap_inuse_bytes', 'unrelated_total']));
    expect(charts.map(({ spec, candidate }) => [spec.chart, candidate.metric])).toEqual([
      ['goroutines', 'go_goroutines'],
      ['heap', 'go_memstats_heap_inuse_bytes'],
    ]);
  });

  it('turns rows into a line per instance', () => {
    const series = seriesOfResult(
      {
        columns: ['_timestamp', 'value', 'service.instance.id'],
        rows: [
          [1, 0.5, 'a'],
          [2, 0.7, 'a'],
          [1, 0.2, 'b'],
          [2, 'NaN', 'b'],
        ],
        scannedRows: 4,
        tookMs: 1,
      },
      'percentunit',
    );
    expect(series.map((entry) => [entry.name, entry.timestamps, entry.data, entry.unit])).toEqual([
      ['a', [1, 2], [0.5, 0.7], 'percentunit'],
      ['b', [1, 2], [0.2, null], 'percentunit'],
    ]);
  });

  it('names an instance by the first label that says one', () => {
    expect(instanceOf({ 'host.name': 'h1', 'service.instance.id': 'i1' })).toBe('i1');
    expect(instanceOf({ 'k8s.pod.name': 'p1', 'host.name': 'h1' })).toBe('p1');
    expect(instanceOf({ 'node.id': 'n1' })).toBe('n1');
    expect(instanceOf({})).toBeNull();
  });
});
