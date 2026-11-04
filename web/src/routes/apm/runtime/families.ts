/**
 * The runtime metrics a service may send, a family for each runtime: the
 * process figures every language has, then the runtime of the language
 * (JVM, Go, Node.js, .NET, CPython, Ruby, PHP, Rust's Tokio, the BEAM of
 * Erlang and Elixir). Each chart lists the names its figure goes by, in
 * OpenTelemetry's conventions as stored here (dots to underscores, the unit
 * appended, `_total` after a counter) and in the Prometheus clients each
 * runtime commonly uses; the first the service sends is drawn.
 *
 * A statement writes `{{metric}}` for the metric with the service's matchers,
 * `{{metric,label="value"}}` for more of them, and `BY` for the labels that
 * tell one instance from another.
 */

/** The labels an instance is told apart by, in the order APM tells them apart. */
export const INSTANCE_LABELS = ['service.instance.id', 'k8s.pod.name', 'host.name'] as const;

export type RuntimeFamilyKey = 'process' | 'jvm' | 'go' | 'nodejs' | 'dotnet' | 'python' | 'ruby' | 'php' | 'rust' | 'beam';

export type RuntimeChart =
  | 'cpu'
  | 'memory'
  | 'threads'
  | 'file_descriptors'
  | 'heap'
  | 'non_heap'
  | 'gc_time'
  | 'gc_count'
  | 'classes'
  | 'goroutines'
  | 'scheduler'
  | 'event_loop_delay'
  | 'event_loop_utilization'
  | 'thread_pool_queue'
  | 'exceptions'
  | 'heap_slots'
  | 'allocations'
  | 'workers'
  | 'queue'
  | 'slow_requests'
  | 'opcache'
  | 'tasks'
  | 'busy'
  | 'processes'
  | 'run_queue'
  | 'reductions';

export interface RuntimeCandidate {
  /** The metric whose presence says this candidate applies. */
  metric: string;
  statement: string;
}

export interface RuntimeChartSpec {
  chart: RuntimeChart;
  unit: string;
  candidates: RuntimeCandidate[];
}

export interface RuntimeFamily {
  key: RuntimeFamilyKey;
  /** The `telemetry.sdk.language` values whose services run on it. */
  languages: readonly string[];
  /** The name fragments the metric catalog is searched by for it. */
  prefixes: readonly string[];
  charts: RuntimeChartSpec[];
}

const sel = (metric: string, matchers = '') => `{{${metric}${matchers ? `,${matchers}` : ''}}}`;
const by = (aggregate: 'sum' | 'avg' | 'max', inner: string) => `${aggregate} by BY (${inner})`;
const rate = (inner: string) => `rate(${inner}[5m])`;

/** A gauge, one line per instance. */
const gauge = (metric: string, aggregate: 'sum' | 'avg' | 'max' = 'sum', matchers = ''): RuntimeCandidate => ({
  metric,
  statement: by(aggregate, sel(metric, matchers)),
});
/** A counter as a rate a second, one line per instance. */
const perSecond = (metric: string, matchers = ''): RuntimeCandidate => ({
  metric,
  statement: by('sum', rate(sel(metric, matchers))),
});
/** The mean of a histogram or summary over the last five minutes: its sum over its count. */
const mean = (base: string): RuntimeCandidate => ({
  metric: `${base}_sum`,
  statement: `${by('sum', rate(sel(`${base}_sum`)))} / ${by('sum', rate(sel(`${base}_count`)))}`,
});
/** A gauge under two conventions of one name: the matchers say which part of it. */
const either = (metric: string, first: string, second: string): RuntimeCandidate => ({
  metric,
  statement: `(${by('sum', sel(metric, first))}) or (${by('sum', sel(metric, second))})`,
});

export const RUNTIME_FAMILIES: readonly RuntimeFamily[] = [
  {
    key: 'process',
    languages: [],
    prefixes: ['process_'],
    charts: [
      {
        chart: 'cpu',
        unit: 'percentunit',
        candidates: [
          gauge('process_cpu_utilization_ratio', 'avg'),
          perSecond('process_cpu_time_seconds_total'),
          perSecond('process_cpu_seconds_total'),
        ],
      },
      {
        chart: 'memory',
        unit: 'bytes',
        candidates: [gauge('process_memory_usage_bytes', 'avg'), gauge('process_resident_memory_bytes', 'avg')],
      },
      { chart: 'threads', unit: '', candidates: [gauge('process_thread_count'), gauge('process_threads')] },
      {
        chart: 'file_descriptors',
        unit: '',
        candidates: [gauge('process_open_file_descriptor_count'), gauge('process_open_fds')],
      },
    ],
  },
  {
    key: 'jvm',
    languages: ['java', 'kotlin', 'scala', 'clojure'],
    prefixes: ['jvm_'],
    charts: [
      { chart: 'cpu', unit: 'percentunit', candidates: [gauge('jvm_cpu_recent_utilization_ratio', 'avg'), perSecond('jvm_cpu_time_seconds_total')] },
      {
        chart: 'heap',
        unit: 'bytes',
        candidates: [either('jvm_memory_used_bytes', '"jvm.memory.type"="heap"', 'area="heap"')],
      },
      {
        chart: 'non_heap',
        unit: 'bytes',
        candidates: [either('jvm_memory_used_bytes', '"jvm.memory.type"="non_heap"', 'area="nonheap"')],
      },
      { chart: 'gc_time', unit: 's', candidates: [perSecond('jvm_gc_duration_seconds_sum'), perSecond('jvm_gc_pause_seconds_sum')] },
      { chart: 'gc_count', unit: '', candidates: [perSecond('jvm_gc_duration_seconds_count'), perSecond('jvm_gc_pause_seconds_count')] },
      { chart: 'threads', unit: '', candidates: [gauge('jvm_thread_count'), gauge('jvm_threads_live_threads')] },
      { chart: 'classes', unit: '', candidates: [gauge('jvm_class_count'), gauge('jvm_classes_loaded_classes')] },
    ],
  },
  {
    key: 'go',
    languages: ['go'],
    prefixes: ['go_', 'process_runtime_go_'],
    charts: [
      { chart: 'goroutines', unit: '', candidates: [gauge('go_goroutine_count'), gauge('go_goroutines'), gauge('process_runtime_go_goroutines')] },
      {
        chart: 'heap',
        unit: 'bytes',
        candidates: [gauge('go_memory_used_bytes'), gauge('go_memstats_heap_inuse_bytes'), gauge('process_runtime_go_mem_heap_inuse_bytes')],
      },
      { chart: 'gc_time', unit: 's', candidates: [perSecond('go_gc_duration_seconds_sum')] },
      { chart: 'gc_count', unit: '', candidates: [perSecond('go_gc_duration_seconds_count'), perSecond('process_runtime_go_gc_count_total')] },
      { chart: 'scheduler', unit: 's', candidates: [mean('go_schedule_duration_seconds'), mean('go_sched_latencies_seconds')] },
      { chart: 'threads', unit: '', candidates: [gauge('go_threads'), gauge('go_processor_limit')] },
    ],
  },
  {
    key: 'nodejs',
    languages: ['nodejs', 'javascript', 'typescript'],
    prefixes: ['v8js_', 'nodejs_'],
    charts: [
      { chart: 'heap', unit: 'bytes', candidates: [gauge('v8js_memory_heap_used_bytes'), gauge('nodejs_heap_size_used_bytes')] },
      { chart: 'gc_time', unit: 's', candidates: [perSecond('v8js_gc_duration_seconds_sum'), perSecond('nodejs_gc_duration_seconds_sum')] },
      {
        chart: 'event_loop_delay',
        unit: 's',
        candidates: [gauge('nodejs_eventloop_delay_p99_seconds', 'max'), gauge('nodejs_eventloop_lag_p99_seconds', 'max')],
      },
      { chart: 'event_loop_utilization', unit: 'percentunit', candidates: [gauge('nodejs_eventloop_utilization_ratio', 'avg')] },
      { chart: 'tasks', unit: '', candidates: [gauge('nodejs_active_handles_total'), gauge('nodejs_active_handles')] },
    ],
  },
  {
    key: 'dotnet',
    languages: ['dotnet'],
    prefixes: ['dotnet_', 'process_runtime_dotnet_'],
    charts: [
      { chart: 'cpu', unit: 'percentunit', candidates: [perSecond('dotnet_process_cpu_time_seconds_total')] },
      {
        chart: 'heap',
        unit: 'bytes',
        candidates: [gauge('dotnet_gc_last_collection_heap_size_bytes'), gauge('process_runtime_dotnet_gc_heap_size_bytes')],
      },
      { chart: 'gc_time', unit: 's', candidates: [perSecond('dotnet_gc_pause_time_seconds_total')] },
      { chart: 'gc_count', unit: '', candidates: [perSecond('dotnet_gc_collections_total'), perSecond('process_runtime_dotnet_gc_collections_count_total')] },
      {
        chart: 'threads',
        unit: '',
        candidates: [gauge('dotnet_thread_pool_thread_count'), gauge('process_runtime_dotnet_thread_pool_threads_count')],
      },
      {
        chart: 'thread_pool_queue',
        unit: '',
        candidates: [gauge('dotnet_thread_pool_queue_length'), gauge('process_runtime_dotnet_thread_pool_queue_length')],
      },
      { chart: 'exceptions', unit: '', candidates: [perSecond('dotnet_exceptions_total'), perSecond('process_runtime_dotnet_exceptions_count_total')] },
    ],
  },
  {
    key: 'python',
    languages: ['python'],
    prefixes: ['cpython_', 'process_runtime_cpython_', 'python_'],
    charts: [
      { chart: 'cpu', unit: 'percentunit', candidates: [perSecond('process_runtime_cpython_cpu_time_seconds_total')] },
      { chart: 'memory', unit: 'bytes', candidates: [gauge('process_runtime_cpython_memory_bytes', 'sum', 'type="rss"')] },
      {
        chart: 'gc_count',
        unit: '',
        candidates: [
          perSecond('cpython_gc_collections_total'),
          perSecond('process_runtime_cpython_gc_count_total'),
          perSecond('python_gc_collections_total'),
        ],
      },
      { chart: 'threads', unit: '', candidates: [gauge('process_runtime_cpython_thread_count')] },
    ],
  },
  {
    key: 'ruby',
    languages: ['ruby'],
    prefixes: ['ruby_', 'puma_'],
    charts: [
      { chart: 'memory', unit: 'bytes', candidates: [gauge('ruby_rss')] },
      { chart: 'heap_slots', unit: '', candidates: [gauge('ruby_heap_live_slots')] },
      { chart: 'gc_count', unit: '', candidates: [perSecond('ruby_major_gc_ops_total'), perSecond('ruby_gc_count_total')] },
      { chart: 'allocations', unit: '', candidates: [perSecond('ruby_allocated_objects_total')] },
      { chart: 'threads', unit: '', candidates: [gauge('puma_running_threads'), gauge('puma_running')] },
      { chart: 'queue', unit: '', candidates: [gauge('puma_backlog')] },
    ],
  },
  {
    key: 'php',
    languages: ['php'],
    prefixes: ['phpfpm_', 'php_'],
    charts: [
      { chart: 'workers', unit: '', candidates: [gauge('phpfpm_active_processes')] },
      { chart: 'queue', unit: '', candidates: [gauge('phpfpm_listen_queue')] },
      { chart: 'slow_requests', unit: '', candidates: [perSecond('phpfpm_slow_requests')] },
      { chart: 'opcache', unit: 'bytes', candidates: [gauge('php_opcache_memory_usage_used_memory')] },
    ],
  },
  {
    key: 'rust',
    languages: ['rust'],
    prefixes: ['tokio_'],
    charts: [
      { chart: 'tasks', unit: '', candidates: [gauge('tokio_num_alive_tasks'), gauge('tokio_runtime_alive_tasks'), gauge('tokio_alive_tasks')] },
      { chart: 'queue', unit: '', candidates: [gauge('tokio_global_queue_depth'), gauge('tokio_runtime_global_queue_depth')] },
      { chart: 'workers', unit: '', candidates: [gauge('tokio_workers_count'), gauge('tokio_runtime_workers_count')] },
      {
        chart: 'busy',
        unit: 'percentunit',
        candidates: [perSecond('tokio_total_busy_duration_seconds_total'), perSecond('tokio_runtime_worker_busy_duration_seconds_total')],
      },
    ],
  },
  {
    key: 'beam',
    languages: ['erlang', 'elixir'],
    prefixes: ['erlang_vm_'],
    charts: [
      { chart: 'processes', unit: '', candidates: [gauge('erlang_vm_process_count')] },
      { chart: 'memory', unit: 'bytes', candidates: [gauge('erlang_vm_memory_bytes_total')] },
      {
        chart: 'run_queue',
        unit: '',
        candidates: [gauge('erlang_vm_statistics_run_queues_length'), gauge('erlang_vm_statistics_run_queues_length_total')],
      },
      { chart: 'gc_count', unit: '', candidates: [perSecond('erlang_vm_statistics_garbage_collection_number_of_gcs')] },
      { chart: 'reductions', unit: '', candidates: [perSecond('erlang_vm_statistics_reductions_total')] },
    ],
  },
];

/**
 * The families a service's runtime charts come from: the process's figures,
 * then the runtime of each language the service reports. A service that does
 * not say its language gets every family the catalog has metrics of.
 */
export function familiesFor(languages: readonly string[], catalog: ReadonlySet<string>): RuntimeFamily[] {
  const spoken = new Set(languages.map((language) => language.toLowerCase()));
  const named = RUNTIME_FAMILIES.filter((family) => family.languages.some((language) => spoken.has(language)));
  const runtimes =
    named.length > 0
      ? named
      : RUNTIME_FAMILIES.filter(
          (family) => family.key !== 'process' && family.charts.some((chart) => chart.candidates.some((candidate) => catalog.has(candidate.metric))),
        );

  return [RUNTIME_FAMILIES[0]!, ...runtimes];
}

/** The charts of a family the catalog has a metric for, each with the first candidate it has. */
export function chartsOf(family: RuntimeFamily, catalog: ReadonlySet<string>): Array<{ spec: RuntimeChartSpec; candidate: RuntimeCandidate }> {
  return family.charts.flatMap((spec) => {
    const candidate = spec.candidates.find((entry) => catalog.has(entry.metric));
    return candidate ? [{ spec, candidate }] : [];
  });
}
