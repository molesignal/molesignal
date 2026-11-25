/**
 * MoleSignal Node Agent in standalone token mode: OTLP gRPC on 4317 for metrics and events, the
 * HTTP API for the fallback and for profiles. The files named here ship with the agent's release
 * (`deploy/systemd/`, `deploy/kubernetes/node-agent.yaml`).
 */

import { ENDPOINT, ENDPOINT_HOST, ENDPOINT_SCHEME, TOKEN, type Source } from './types';

/** The OTLP gRPC address the agent sends to; the HTTP API is the access address itself. */
const GRPC_ENDPOINT = `${ENDPOINT_SCHEME}://${ENDPOINT_HOST}:4317`;

export const NODE_AGENT_SOURCE: Source = {
  id: 'node-agent',
  category: 'recommended',
  signals: ['metrics', 'logs', 'profiles'],
  steps: [
    {
      code: {
        lang: 'bash',
        content: `# On each host, as root, in the unpacked agent release
sh deploy/systemd/install.sh install ./molesignal-node-agent --no-start`,
      },
    },
    {
      code: {
        lang: 'bash',
        content: `cat > /etc/molesignal-node-agent/agent.env <<'ENV'
MS_ENDPOINT=${GRPC_ENDPOINT}
MS_HTTP_ENDPOINT=${ENDPOINT}
MS_TOKEN_PATH=/etc/molesignal-node-agent/token
ENV
printf '%s' '${TOKEN}' > /etc/molesignal-node-agent/token
chmod 0600 /etc/molesignal-node-agent/token
systemctl enable --now molesignal-node-agent`,
      },
    },
    {
      code: {
        lang: 'bash',
        content: `kubectl apply -f deploy/kubernetes/node-agent.yaml
printf '%s' '${TOKEN}' > agent.token
kubectl -n molesignal create secret generic molesignal-node-agent-token --from-file=token=./agent.token
kubectl -n molesignal patch configmap molesignal-node-agent --type merge \\
  -p '{"data":{"MS_ENDPOINT":"${GRPC_ENDPOINT}","MS_HTTP_ENDPOINT":"${ENDPOINT}"}}'
kubectl -n molesignal rollout restart daemonset/molesignal-node-agent`,
      },
    },
    {
      verification: true,
      code: {
        lang: 'bash',
        content: `molesignal-node-agent doctor
# Kubernetes
kubectl -n molesignal exec ds/molesignal-node-agent -- molesignal-node-agent doctor`,
      },
    },
  ],
};
