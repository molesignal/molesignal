import type { AuthMode, ConnectorKind, FieldSpec, KindSpec } from './kinds';

/*
 * The sources that pull a cloud's monitoring metrics on a schedule
 * (`infra/connectors/{aws,azure,gcp,aliyun,tencent,huawei}`): each reads
 * whole periods and writes one sample per statistic to its metrics stream.
 */

/** Where a metrics source writes its samples, and the periods it reads. */
const METRICS_STREAM = (example: string): FieldSpec => ({ key: 'metricsStream', stream: 'metrics', example });
const PERIOD: FieldSpec = { key: 'period', example: '300' };
const DELAY = (example: string): FieldSpec => ({ key: 'delay', example });

/** The cloud metric kinds; AWS signs in the ways every AWS kind does. */
export function cloudMetricKinds(awsAuth: readonly AuthMode[]): KindSpec[] {
  return [
    {
      kind: 'aws_cloudwatch_metrics',
      direction: 'source',
      transport: 'pull',
      auth: awsAuth,
      fields: [
        { key: 'region', required: true, example: 'us-east-1' },
        { key: 'namespace', required: true, example: 'AWS/EC2' },
        { key: 'metrics', example: 'CPUUtilization, NetworkIn' },
        { key: 'statistics', example: 'Average, Maximum' },
        METRICS_STREAM('cloudwatch'),
        PERIOD,
        DELAY('600'),
      ],
    },
    {
      kind: 'azure_monitor_metrics',
      direction: 'source',
      transport: 'pull',
      auth: ['client_secret'],
      fields: [
        { key: 'subscriptionId', required: true, example: '00000000-0000-0000-0000-000000000000' },
        { key: 'region', required: true, example: 'eastus' },
        { key: 'resourceType', required: true, example: 'Microsoft.Compute/virtualMachines' },
        { key: 'metrics', required: true, example: 'Percentage CPU, Network In Total' },
        { key: 'aggregations', example: 'average, maximum' },
        { key: 'filter', example: "LUN eq '*'" },
        { key: 'top', example: '100' },
        METRICS_STREAM('azure'),
        PERIOD,
        DELAY('600'),
      ],
    },
    {
      kind: 'gcp_monitoring_metrics',
      direction: 'source',
      transport: 'pull',
      auth: ['service_account_key'],
      fields: [
        { key: 'projectId', example: 'my-project' },
        { key: 'metrics', required: true, multiline: true, example: 'compute.googleapis.com/instance/cpu/utilization' },
        { key: 'filter', example: 'resource.labels.zone = "us-central1-a"' },
        METRICS_STREAM('gcp'),
        PERIOD,
        DELAY('300'),
      ],
    },
    {
      kind: 'aliyun_cms_metrics',
      direction: 'source',
      transport: 'pull',
      auth: ['access_key'],
      fields: [
        { key: 'region', required: true, example: 'cn-hangzhou' },
        { key: 'namespace', required: true, example: 'acs_ecs_dashboard' },
        { key: 'metrics', required: true, example: 'CPUUtilization, memory_usedutilization' },
        { key: 'statistics', example: 'Average, Maximum' },
        { key: 'dimensions', multiline: true, example: '[{"instanceId": "i-bp1..."}]' },
        METRICS_STREAM('aliyun'),
        PERIOD,
        DELAY('300'),
      ],
    },
    {
      kind: 'tencent_monitor_metrics',
      direction: 'source',
      transport: 'pull',
      auth: ['access_key'],
      fields: [
        { key: 'region', required: true, example: 'ap-guangzhou' },
        { key: 'namespace', required: true, example: 'QCE/CVM' },
        { key: 'metrics', required: true, example: 'CpuUsage, MemUsage' },
        { key: 'statistics', example: 'avg, max' },
        { key: 'instances', multiline: true, example: 'InstanceId=ins-abc123' },
        METRICS_STREAM('tencentcloud'),
        PERIOD,
        DELAY('300'),
      ],
    },
    {
      kind: 'huawei_ces_metrics',
      direction: 'source',
      transport: 'pull',
      auth: ['access_key'],
      fields: [
        { key: 'region', required: true, example: 'cn-north-4' },
        { key: 'projectId', required: true, example: '0123456789abcdef0123456789abcdef' },
        { key: 'namespace', required: true, example: 'SYS.ECS' },
        { key: 'metrics', required: true, example: 'cpu_util, mem_util' },
        { key: 'statistics', example: 'average, max' },
        METRICS_STREAM('huaweicloud'),
        { key: 'period', choices: ['300', '1200', '3600', '14400', '86400'] },
        DELAY('300'),
      ],
    },
  ];
}

/** The metrics stream each metrics source writes to when none is named, as the runner falls back. */
export const METRICS_DEFAULTS: Partial<Record<ConnectorKind, string>> = {
  aws_cloudwatch_metrics: 'cloudwatch',
  azure_monitor_metrics: 'azure',
  gcp_monitoring_metrics: 'gcp',
  aliyun_cms_metrics: 'aliyun',
  tencent_monitor_metrics: 'tencentcloud',
  huawei_ces_metrics: 'huaweicloud',
};
