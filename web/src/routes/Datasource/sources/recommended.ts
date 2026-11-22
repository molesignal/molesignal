/** The sources most workspaces start from: hosts, clusters, clouds, RUM and profiling. */

import { MOBILE_RUM_SOURCES } from './mobileRum';
import { ENDPOINT, TOKEN, type Source } from './types';

export const RECOMMENDED_SOURCES: Source[] = [
  {
    id: 'kubernetes',
    category: 'recommended',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `kubectl create namespace telemetry
kubectl create configmap fluent-bit-config -n telemetry \\
  --from-file=fluent-bit.conf=./fluent-bit.conf`,
        },
      },
      {
        code: {
          lang: 'yaml',
          content: `apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: fluent-bit
  namespace: telemetry
spec:
  selector:
    matchLabels: { app: fluent-bit }
  template:
    metadata:
      labels: { app: fluent-bit }
    spec:
      serviceAccountName: fluent-bit
      containers:
      - name: fluent-bit
        image: fluent/fluent-bit:3.0
        env:
        - name: MS_ENDPOINT
          value: "${ENDPOINT}/api/v1/intake/logs/default"
        - name: MS_TOKEN
          valueFrom:
            secretKeyRef: { name: molesignal-token, key: token }
        volumeMounts:
        - { name: varlog,            mountPath: /var/log }
        - { name: varlibdockercontainers, mountPath: /var/lib/docker/containers, readOnly: true }
        - { name: config,            mountPath: /fluent-bit/etc }
      volumes:
      - { name: varlog,            hostPath: { path: /var/log } }
      - { name: varlibdockercontainers, hostPath: { path: /var/lib/docker/containers } }
      - { name: config,            configMap: { name: fluent-bit-config } }`,
        },
      },
      {
        verification: true,
        code: { lang: 'bash', content: 'kubectl logs -n telemetry -l app=fluent-bit --tail=20' },
      },
    ],
  },
  {
    id: 'linux',
    category: 'recommended',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `curl --proto '=https' --tlsv1.2 -sSf https://sh.vector.dev | bash
sudo systemctl enable --now vector`,
        },
      },
      {
        code: {
          lang: 'yaml',
          content: `sources:
  host_journald:
    type: journald
    current_boot_only: true
  host_metrics:
    type: host_metrics
    scrape_interval_secs: 15

sinks:
  molesignal_logs:
    type: http
    inputs: [host_journald]
    uri: ${ENDPOINT}/api/v1/intake/logs/default
    encoding: { codec: json }
    auth: { strategy: bearer, token: ${TOKEN} }`,
        },
      },
    ],
  },
  {
    id: 'windows',
    category: 'recommended',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'powershell',
          content: `Invoke-WebRequest -Uri https://artifacts.elastic.co/.../winlogbeat-8.15.0-windows-x86_64.zip -OutFile wb.zip
Expand-Archive wb.zip -DestinationPath C:\\Program Files\\winlogbeat`,
        },
      },
      {
        code: {
          lang: 'yaml',
          content: `winlogbeat.event_logs:
  - name: Application
  - name: System
  - name: Security

output.http:
  hosts: ["${ENDPOINT}/api/v1/intake/logs/default"]
  headers:
    Authorization: "Bearer ${TOKEN}"`,
        },
      },
    ],
  },
  {
    id: 'aws',
    category: 'recommended',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `aws firehose create-delivery-stream \\
  --delivery-stream-name molesignal-logs \\
  --http-endpoint-destination-configuration \\
    "EndpointConfiguration={Url=${ENDPOINT}/api/v1/_kinesis_firehose,AccessKey=<connector-push-token>}"`,
        },
      },
      {
        code: {
          lang: 'bash',
          content: `aws logs put-subscription-filter \\
  --log-group-name /aws/lambda/my-fn \\
  --filter-name molesignal \\
  --filter-pattern "" \\
  --destination-arn arn:aws:firehose:us-east-1:123:deliverystream/molesignal-logs`,
        },
      },
    ],
  },
  {
    id: 'gcp',
    category: 'recommended',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `gcloud logging sinks create molesignal-sink \\
  pubsub.googleapis.com/projects/$PROJECT/topics/molesignal-logs \\
  --log-filter='resource.type="k8s_container"'`,
        },
      },
      {},
    ],
  },
  {
    id: 'azure',
    category: 'recommended',
    signals: ['logs'],
    steps: [
      {},
    ],
  },
  ...MOBILE_RUM_SOURCES,

  {
    id: 'continuous-profiling',
    category: 'recommended',
    signals: ['profiles'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `OTEL_EXPORTER_OTLP_PROFILES_ENDPOINT=${ENDPOINT}/api/v1/profiles/otlp`,
        },
      },
      {
        code: {
          lang: 'bash',
          content: `curl -X POST "${ENDPOINT}/api/v1/profiles/intake?name=my-service&format=pprof" \\
  -H "Authorization: Bearer ${TOKEN}" \\
  --data-binary @profile.pprof`,
        },
      },
      {
        code: {
          lang: 'bash',
          content: `curl -X POST "${ENDPOINT}/api/v1/profiles/upload?service=my-service&type=cpu" \\
  -H "Authorization: Bearer ${TOKEN}" \\
  --data-binary @cpu.pprof`,
        },
      },
    ],
  },
];
