export type MonitorKind =
  | 'http'
  | 'tcp'
  | 'ssh'
  | 'dns'
  | 'icmp'
  | 'tls'
  | 'grpc'
  | 'browser'
  | 'heartbeat';

export type MonitorLifecycle = 'draft' | 'active' | 'paused' | 'archived';
export type MonitorState = 'healthy' | 'flaky' | 'degraded' | 'failing' | 'unknown';
export type ProbeOutcome = MonitorState | 'skipped';

export type ValueSource =
  | { source: 'literal'; value: string }
  | { source: 'variable'; name: string }
  | { source: 'secret'; reference: string; secretId: string };

export type AssertionSeverity = 'warning' | 'critical';

export type AssertionOperator =
  | { operator: 'equals'; expected: string }
  | { operator: 'not_equals'; expected: string }
  | { operator: 'contains'; expected: string }
  | { operator: 'not_contains'; expected: string }
  | { operator: 'matches'; pattern: string }
  | { operator: 'greater_than'; expected: number }
  | { operator: 'less_than'; expected: number }
  | { operator: 'json_schema'; schema: unknown }
  | { operator: 'exists' };

export interface MonitorAssertion {
  id: string;
  name: string;
  source: string;
  severity: AssertionSeverity;
  operator: AssertionOperator;
}

export interface Extraction {
  variable: string;
  source: string;
  expression: string;
  required: boolean;
}

export interface HeaderValue {
  name: string;
  value: ValueSource;
}

/** What TLS negotiates, or HTTP/1.1 or HTTP/2 only (over plain HTTP by prior knowledge). */
export type HttpVersion = 'auto' | 'http1' | 'http2';

/**
 * The private key of a client certificate: a new one in PEM, or the one a revision holds. The
 * server seals a key and never sends it back; a revision shows its key as kept.
 */
export type ClientKey = { state: 'new'; pem: string } | { state: 'kept'; revisionId: string };

/** A host name reached at chosen addresses, as a hosts file would. */
export interface HostBinding {
  name: string;
  addresses: string[];
}

export interface HttpStep {
  id: string;
  name: string;
  method: string;
  url: ValueSource;
  headers: HeaderValue[];
  query: HeaderValue[];
  body?: ValueSource;
  extractions: Extraction[];
  assertions: MonitorAssertion[];
}

export interface BrowserStep {
  id: string;
  name: string;
  action: Record<string, unknown> & { action: string };
}

export type MonitorSpec =
  | {
      kind: 'http';
      configuration: {
        steps: HttpStep[];
        followRedirects: boolean;
        maxRedirects: number;
        verifyTls: boolean;
        httpVersion?: HttpVersion;
        clientCertificate?: { certificate: string; privateKey: ClientKey };
        dnsServer?: string;
        hosts?: HostBinding[];
      };
    }
  | {
      kind: 'browser';
      configuration: {
        steps: BrowserStep[];
        viewport: { width: number; height: number };
        userAgent?: string;
        captureScreenshotOnFailure: boolean;
        captureHarOnFailure: boolean;
        captureTraceOnFailure: boolean;
      };
    }
  | {
      kind: 'tcp';
      configuration: {
        host: ValueSource;
        port: number;
        useTls: boolean;
        serverName?: string;
        send?: ValueSource;
        expectRegex?: string;
        assertions: MonitorAssertion[];
      };
    }
  | {
      kind: 'ssh';
      configuration: {
        host: ValueSource;
        port: number;
        expectedIdentificationRegex?: string;
        authentication?:
          | { kind: 'password'; username: ValueSource; password: ValueSource }
          | {
              kind: 'public_key';
              username: ValueSource;
              privateKey: ValueSource;
              passphrase?: ValueSource;
            };
        command?: ValueSource;
        expectedOutputRegex?: string;
        expectedExitStatus?: number;
        expectedHostKeySha256?: string;
      };
    }
  | {
      kind: 'dns';
      configuration: {
        name: string;
        recordType: string;
        resolver?: string;
        requireDnssec: boolean;
        expectedValues: string[];
        expectedRcode?: string;
        assertions: MonitorAssertion[];
      };
    }
  | {
      kind: 'icmp';
      configuration: {
        host: string;
        count: number;
        intervalMillis: number;
        maxPacketLossRatio: number;
        maxMeanRttMillis?: number;
      };
    }
  | {
      kind: 'tls';
      configuration: {
        host: string;
        port: number;
        serverName?: string;
        minimumDaysRemaining: number;
        expectedSans: string[];
        expectedIssuerRegex?: string;
        minimumProtocol?: string;
      };
    }
  | {
      kind: 'grpc';
      configuration: {
        endpoint: string;
        useTls: boolean;
        serverName?: string;
        metadata: HeaderValue[];
        call:
          | { kind: 'health'; service: string }
          | {
              kind: 'unary';
              service: string;
              method: string;
              descriptorSetBase64?: string;
              useReflection: boolean;
              requestJson: string;
            };
        assertions: MonitorAssertion[];
      };
    }
  | { kind: 'heartbeat' };

export type MonitorSchedule =
  | { kind: 'interval'; everySeconds: number }
  | { kind: 'cron'; expression: string; timezone: string }
  | { kind: 'heartbeat'; expectedSeconds: number; graceSeconds: number };

export type MultiLocationPolicy =
  | { kind: 'any' }
  | { kind: 'quorum'; required: number }
  | { kind: 'majority' }
  | { kind: 'all' };

export interface SyntheticMonitor {
  id: string;
  organizationId: string;
  name: string;
  description: string;
  kind: MonitorKind;
  lifecycle: MonitorLifecycle;
  state: MonitorState;
  teamId?: string;
  tags: string[];
  activeRevisionId?: string;
  draftRevisionId?: string;
  nextDueAt?: number;
  createdBy: string;
  createdAt: number;
  updatedAt: number;
  archivedAt?: number;
}

export interface MonitorRevision {
  id: string;
  organizationId: string;
  monitorId: string;
  number: number;
  spec: MonitorSpec;
  schedule: MonitorSchedule;
  timeoutMillis: number;
  maxRetries: number;
  consecutiveFailures: number;
  consecutiveRecoveries: number;
  freshnessSeconds: number;
  locationPolicy: MultiLocationPolicy;
  locationIds: string[];
  escalationPolicyId?: string;
  alertOnDegraded: boolean;
  alertOnFlaky: boolean;
  /** An incident opens when enough locations stop getting results. */
  alertOnNoData: boolean;
  /** HTTP requests and gRPC calls carry a W3C trace context. */
  injectTraceContext: boolean;
  /** A run that cannot reach its target traces the route to it. */
  tracerouteOnFailure: boolean;
  lastTestResultId?: string;
  lastTestPassedAt?: number;
  createdBy: string;
  createdAt: number;
  contentHash: string;
}

export interface ActiveMonitorRevision {
  monitor: SyntheticMonitor;
  revision: MonitorRevision;
}

export interface MonitorDetail {
  monitor: SyntheticMonitor;
  revisions: MonitorRevision[];
  /** Where the published revision is missing runs. */
  missingRuns?: MissingRuns;
}

/**
 * The locations of a check that missed as many runs in a row as fail a
 * location (runs no Agent took in time), and whether they are as many as its
 * location policy needs: then the check has no results.
 */
export interface MissingRuns {
  noData: boolean;
  locations: { locationId: string; missedRuns: number }[];
}

export interface CreateMonitorInput {
  name: string;
  description: string;
  spec: MonitorSpec;
  schedule: MonitorSchedule;
  timeoutMillis: number;
  maxRetries: number;
  consecutiveFailures: number;
  consecutiveRecoveries: number;
  freshnessSeconds: number;
  locationPolicy: MultiLocationPolicy;
  locationIds: string[];
  teamId?: string;
  tags: string[];
  escalationPolicyId?: string;
  alertOnDegraded: boolean;
  alertOnFlaky: boolean;
  alertOnNoData: boolean;
  injectTraceContext: boolean;
  tracerouteOnFailure: boolean;
}

/**
 * Where one attempt spent its time, in microseconds.
 *
 * Each phase is a duration of its own, not an offset from the start of the
 * attempt, summed over the connections the attempt opened, so the phases come to
 * at most `total_micros`. A phase the probe did not time is absent.
 * `TimingBreakdown` in `proto/probe/v1/probe.proto` is the full definition.
 */
export interface TimingBreakdown {
  dnsTime?: number;
  connectTime?: number;
  tlsTime?: number;
  firstByteTime?: number;
  totalTime?: number;
}

export interface ProbeAttempt {
  number: number;
  startedAt: number;
  finishedAt: number;
  outcome: ProbeOutcome;
  timing: TimingBreakdown;
  errorCategory?: string;
  errorMessage?: string;
  boundedResponseExcerpt?: number[];
  metadata: Record<string, string>;
  evidence?: ProbeStepEvidence[];
}

export interface ProbeStepEvidence {
  stepId: string;
  name: string;
  action: string;
  startedAt: number;
  finishedAt: number;
  outcome: ProbeOutcome;
  errorCategory?: string;
  errorMessage?: string;
  metadata: Record<string, string>;
}

export interface AssertionObservation {
  assertionId: string;
  severity: AssertionSeverity;
  passed: boolean;
  actual?: string;
  message?: string;
}

export interface SyntheticResultArtifact {
  id: string;
  name: string;
  kind: 'screenshot' | 'har' | string;
  contentType: string;
  contentLength: number;
  sha256: string;
  expiresAt: number;
  createdAt: number;
}

export interface SyntheticResult {
  id: string;
  organizationId: string;
  monitorId: string;
  monitorRevisionId: string;
  locationId: string;
  agentId?: string;
  taskId: string;
  isTest: boolean;
  resultSequence?: number;
  scheduledAt: number;
  startedAt: number;
  finishedAt: number;
  receivedAt: number;
  outcome: ProbeOutcome;
  attempts: ProbeAttempt[];
  assertions: AssertionObservation[];
  artifacts?: SyntheticResultArtifact[];
  secretVersions: Record<string, number>;
  protocolVersion: number;
  metadata: Record<string, string>;
}

export interface SyntheticResultPage {
  items: SyntheticResult[];
  total: number;
  page: number;
  perPage: number;
}

/**
 * How a set of runs went. A run that passed, flapped or slowed was available; one that failed was
 * not; one that could not be judged or never ran (`undecided`) counts toward neither.
 */
export interface AvailabilitySummary {
  runs: number;
  available: number;
  failing: number;
  undecided: number;
  /** Available runs of those judged; null when no run was. */
  availability: number | null;
  p50Ms: number | null;
  p95Ms: number | null;
  maxMs: number | null;
}

/** How the checks did over a range, from the `_synthetic_results` stream. Times in microseconds. */
export interface SyntheticAvailability {
  from: number;
  to: number;
  /** The step the range is cut into. */
  step: number;
  total: AvailabilitySummary;
  checks: Array<AvailabilitySummary & { checkId: string }>;
  locations: Array<AvailabilitySummary & { locationId: string }>;
  buckets: Array<AvailabilitySummary & { start: number }>;
  /** Each location step by step, when one check is asked about. */
  locationBuckets: Array<AvailabilitySummary & { locationId: string; start: number }>;
}

export type LocationScope = 'platform' | 'organization';
export type LocationExecution = 'embedded' | 'agent_pool';
export type LocationLifecycle = 'active' | 'paused' | 'archived';
export type LocationHealth = 'online' | 'degraded' | 'offline' | 'unknown';

export interface EgressPolicy {
  allowedCidrs: string[];
  deniedCidrs: string[];
  allowedDomains: string[];
  deniedDomains: string[];
  allowedPorts: number[];
  allowPrivateNetworks: boolean;
  allowLoopback: boolean;
}

export interface ProbeLocation {
  id: string;
  organizationId?: string;
  name: string;
  code: string;
  description: string;
  scope: LocationScope;
  execution: LocationExecution;
  lifecycle: LocationLifecycle;
  health: LocationHealth;
  systemManaged: boolean;
  egressPolicy: EgressPolicy;
  createdAt: number;
  updatedAt: number;
}

export interface ProbeAgent {
  id: string;
  organizationId?: string;
  locationId: string;
  name: string;
  hostname: string;
  status: 'registered' | 'online' | 'degraded' | 'offline' | 'draining' | 'revoked';
  agentVersion: string;
  protocolVersion: number;
  /** What the Agent runs beyond the first protocol, from its last heartbeat. */
  featureLevel?: number;
  capabilities: MonitorKind[];
  capacity: {
    maxConcurrent: number;
    maxBrowserConcurrent: number;
    available: number;
    availableBrowser: number;
  };
  labels: Record<string, string>;
  certificateSerial?: string;
  certificateExpiresAt?: number;
  lastHeartbeatAt?: number;
  lastResultSequence: number;
  revokedAt?: number;
  createdAt: number;
  updatedAt: number;
}

export interface UpdateAgentConfigurationInput {
  name: string;
  labels: Record<string, string>;
}

export interface ProbeRegisterInstructions {
  token: {
    id: string;
    organizationId: string;
    locationId: string;
    expiresAt: number;
  };
  command: string;
}

export type ProbeAgentTokenStatus = 'active' | 'disabled';

export interface ProbeAgentToken {
  id: string;
  organizationId: string;
  locationId: string;
  name: string;
  tokenPrefix: string;
  status: ProbeAgentTokenStatus;
  expiresAt?: number;
  lastUsedAt?: number;
  createdBy: string;
  createdAt: number;
  rotatedAt?: number;
  disabledAt?: number;
  updatedAt: number;
}

export interface ProbeAgentTokenInstructions {
  token: ProbeAgentToken;
  agentToken: string;
  command: string;
}

export interface SyntheticSecret {
  id: string;
  organizationId: string;
  name: string;
  description: string;
  currentVersion: number;
  createdBy: string;
  createdAt: number;
  updatedAt: number;
  archivedAt?: number;
}
