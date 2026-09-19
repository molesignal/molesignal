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
  | { source: 'secret'; reference: string; secret_id: string };

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
        follow_redirects: boolean;
        max_redirects: number;
        verify_tls: boolean;
      };
    }
  | {
      kind: 'browser';
      configuration: {
        steps: BrowserStep[];
        viewport: { width: number; height: number };
        user_agent?: string;
        capture_screenshot_on_failure: boolean;
        capture_har_on_failure: boolean;
        capture_trace_on_failure: boolean;
      };
    }
  | {
      kind: 'tcp';
      configuration: {
        host: ValueSource;
        port: number;
        use_tls: boolean;
        server_name?: string;
        send?: ValueSource;
        expect_regex?: string;
        assertions: MonitorAssertion[];
      };
    }
  | {
      kind: 'ssh';
      configuration: {
        host: ValueSource;
        port: number;
        expected_identification_regex?: string;
        authentication?:
          | { kind: 'password'; username: ValueSource; password: ValueSource }
          | {
              kind: 'public_key';
              username: ValueSource;
              private_key: ValueSource;
              passphrase?: ValueSource;
            };
        command?: ValueSource;
        expected_output_regex?: string;
        expected_exit_status?: number;
        expected_host_key_sha256?: string;
      };
    }
  | {
      kind: 'dns';
      configuration: {
        name: string;
        record_type: string;
        resolver?: string;
        require_dnssec: boolean;
        expected_values: string[];
        expected_rcode?: string;
        assertions: MonitorAssertion[];
      };
    }
  | {
      kind: 'icmp';
      configuration: {
        host: string;
        count: number;
        interval_millis: number;
        max_packet_loss_ratio: number;
        max_mean_rtt_millis?: number;
      };
    }
  | {
      kind: 'tls';
      configuration: {
        host: string;
        port: number;
        server_name?: string;
        minimum_days_remaining: number;
        expected_sans: string[];
        expected_issuer_regex?: string;
        minimum_protocol?: string;
      };
    }
  | {
      kind: 'grpc';
      configuration: {
        endpoint: string;
        use_tls: boolean;
        server_name?: string;
        metadata: HeaderValue[];
        call:
          | { kind: 'health'; service: string }
          | {
              kind: 'unary';
              service: string;
              method: string;
              descriptor_set_base64?: string;
              use_reflection: boolean;
              request_json: string;
            };
        assertions: MonitorAssertion[];
      };
    }
  | { kind: 'heartbeat' };

export type MonitorSchedule =
  | { kind: 'interval'; every_seconds: number }
  | { kind: 'cron'; expression: string; timezone: string }
  | { kind: 'heartbeat'; expected_seconds: number; grace_seconds: number };

export type MultiLocationPolicy =
  | { kind: 'any' }
  | { kind: 'quorum'; required: number }
  | { kind: 'majority' }
  | { kind: 'all' };

export interface SyntheticMonitor {
  id: string;
  organization_id: string;
  name: string;
  description: string;
  kind: MonitorKind;
  lifecycle: MonitorLifecycle;
  state: MonitorState;
  team_id?: string;
  tags: string[];
  active_revision_id?: string;
  draft_revision_id?: string;
  next_due_at?: number;
  created_by: string;
  created_at: number;
  updated_at: number;
  archived_at?: number;
}

export interface MonitorRevision {
  id: string;
  organization_id: string;
  monitor_id: string;
  number: number;
  spec: MonitorSpec;
  schedule: MonitorSchedule;
  timeout_millis: number;
  max_retries: number;
  consecutive_failures: number;
  consecutive_recoveries: number;
  freshness_seconds: number;
  location_policy: MultiLocationPolicy;
  location_ids: string[];
  escalation_policy_id?: string;
  alert_on_degraded: boolean;
  alert_on_flaky: boolean;
  last_test_result_id?: string;
  last_test_passed_at?: number;
  created_by: string;
  created_at: number;
  content_hash: string;
}

export interface ActiveMonitorRevision {
  monitor: SyntheticMonitor;
  revision: MonitorRevision;
}

export interface MonitorDetail {
  monitor: SyntheticMonitor;
  revisions: MonitorRevision[];
}

export interface CreateMonitorInput {
  name: string;
  description: string;
  spec: MonitorSpec;
  schedule: MonitorSchedule;
  timeout_millis: number;
  max_retries: number;
  consecutive_failures: number;
  consecutive_recoveries: number;
  freshness_seconds: number;
  location_policy: MultiLocationPolicy;
  location_ids: string[];
  team_id?: string;
  tags: string[];
  escalation_policy_id?: string;
  alert_on_degraded: boolean;
  alert_on_flaky: boolean;
}

export interface TimingBreakdown {
  dns_micros?: number;
  connect_micros?: number;
  tls_micros?: number;
  first_byte_micros?: number;
  total_micros?: number;
}

export interface ProbeAttempt {
  number: number;
  started_at: number;
  finished_at: number;
  outcome: ProbeOutcome;
  timing: TimingBreakdown;
  error_category?: string;
  error_message?: string;
  bounded_response_excerpt?: number[];
  metadata: Record<string, string>;
  evidence?: ProbeStepEvidence[];
}

export interface ProbeStepEvidence {
  step_id: string;
  name: string;
  action: string;
  started_at: number;
  finished_at: number;
  outcome: ProbeOutcome;
  error_category?: string;
  error_message?: string;
  metadata: Record<string, string>;
}

export interface AssertionObservation {
  assertion_id: string;
  severity: AssertionSeverity;
  passed: boolean;
  actual?: string;
  message?: string;
}

export interface SyntheticResultArtifact {
  id: string;
  name: string;
  kind: 'screenshot' | 'har' | string;
  content_type: string;
  content_length: number;
  sha256: string;
  expires_at: number;
  created_at: number;
}

export interface SyntheticResult {
  id: string;
  organization_id: string;
  monitor_id: string;
  monitor_revision_id: string;
  location_id: string;
  agent_id?: string;
  task_id: string;
  is_test: boolean;
  result_sequence?: number;
  scheduled_at: number;
  started_at: number;
  finished_at: number;
  received_at: number;
  outcome: ProbeOutcome;
  attempts: ProbeAttempt[];
  assertions: AssertionObservation[];
  artifacts?: SyntheticResultArtifact[];
  secret_versions: Record<string, number>;
  protocol_version: number;
  metadata: Record<string, string>;
}

export interface SyntheticResultPage {
  items: SyntheticResult[];
  total: number;
  page: number;
  per_page: number;
}

export type LocationScope = 'platform' | 'organization';
export type LocationExecution = 'embedded' | 'agent_pool';
export type LocationLifecycle = 'active' | 'paused' | 'archived';
export type LocationHealth = 'online' | 'degraded' | 'offline' | 'unknown';

export interface EgressPolicy {
  allowed_cidrs: string[];
  denied_cidrs: string[];
  allowed_domains: string[];
  denied_domains: string[];
  allowed_ports: number[];
  allow_private_networks: boolean;
  allow_loopback: boolean;
}

export interface ProbeLocation {
  id: string;
  organization_id?: string;
  name: string;
  code: string;
  description: string;
  scope: LocationScope;
  execution: LocationExecution;
  lifecycle: LocationLifecycle;
  health: LocationHealth;
  system_managed: boolean;
  egress_policy: EgressPolicy;
  created_at: number;
  updated_at: number;
}

export interface ProbeAgent {
  id: string;
  organization_id?: string;
  location_id: string;
  name: string;
  hostname: string;
  status: 'registered' | 'online' | 'degraded' | 'offline' | 'draining' | 'revoked';
  agent_version: string;
  protocol_version: number;
  capabilities: MonitorKind[];
  capacity: {
    max_concurrent: number;
    max_browser_concurrent: number;
    available: number;
    available_browser: number;
  };
  labels: Record<string, string>;
  certificate_serial?: string;
  certificate_expires_at?: number;
  last_heartbeat_at?: number;
  last_result_sequence: number;
  revoked_at?: number;
  created_at: number;
  updated_at: number;
}

export interface UpdateAgentConfigurationInput {
  name: string;
  labels: Record<string, string>;
}

export interface ProbeRegisterInstructions {
  token: {
    id: string;
    organization_id: string;
    location_id: string;
    expires_at: number;
  };
  command: string;
}

export type ProbeAgentTokenStatus = 'active' | 'disabled';

export interface ProbeAgentToken {
  id: string;
  organization_id: string;
  location_id: string;
  name: string;
  token_prefix: string;
  status: ProbeAgentTokenStatus;
  expires_at?: number;
  last_used_at?: number;
  created_by: string;
  created_at: number;
  rotated_at?: number;
  disabled_at?: number;
  updated_at: number;
}

export interface ProbeAgentTokenInstructions {
  token: ProbeAgentToken;
  agent_token: string;
  command: string;
}

export interface SyntheticSecret {
  id: string;
  organization_id: string;
  name: string;
  description: string;
  current_version: number;
  created_by: string;
  created_at: number;
  updated_at: number;
  archived_at?: number;
}
