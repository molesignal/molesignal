/**
 * What to do when Node Agent's data does not arrive as it should. The words of each entry are in
 * the `onboarding` namespace under `datasource_page.node_agent.runbook.items.<id>`; the commands,
 * which do not translate, are here.
 */

export interface RunbookEntry {
  id: 'no_data' | 'unauthorized' | 'grpc_http' | 'no_profiles' | 'float_missing' | 'no_events';
  code: { lang: string; content: string };
}

export const RUNBOOK: readonly RunbookEntry[] = [
  {
    id: 'no_data',
    code: {
      lang: 'bash',
      content: `systemctl status molesignal-node-agent
journalctl -u molesignal-node-agent -n 50 --no-pager
molesignal-node-agent doctor`,
    },
  },
  {
    id: 'unauthorized',
    code: {
      lang: 'bash',
      content: `molesignal-node-agent doctor          # the auth line names the answer of the endpoint
ls -l /etc/molesignal-node-agent/token # -rw------- root root, no trailing newline needed
systemctl restart molesignal-node-agent`,
    },
  },
  {
    id: 'grpc_http',
    code: {
      lang: 'bash',
      content: `grep '^MS_' /etc/molesignal-node-agent/agent.env
# MS_ENDPOINT=<scheme>://<host>:4317       OTLP gRPC: metrics and events
# MS_HTTP_ENDPOINT=<scheme>://<host>:5080  HTTP API: fallback and profiles`,
    },
  },
  {
    id: 'no_profiles',
    code: {
      lang: 'bash',
      content: `echo 'MS_PROFILING_CPU_ENABLED=true' >> /etc/molesignal-node-agent/agent.env
grep '^MS_HTTP_ENDPOINT' /etc/molesignal-node-agent/agent.env
systemctl restart molesignal-node-agent`,
    },
  },
  {
    id: 'float_missing',
    code: {
      lang: 'sql',
      content: `SELECT metric_name, COUNT(*) AS points, MAX(value) AS last_max
FROM "host-metrics"
WHERE metric_name IN ('molesignal_host_loadavg_load1', 'molesignal_host_cpu_util_total')
GROUP BY metric_name`,
    },
  },
  {
    id: 'no_events',
    code: {
      lang: 'bash',
      content: `molesignal-node-agent doctor        # kernel, BTF, tracefs, capabilities
molesignal-node-agent capabilities  # which collectors run on this host`,
    },
  },
];
