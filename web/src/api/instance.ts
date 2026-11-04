import { http } from '@/lib/http';

import type { StreamType } from './streams';

export interface InstanceInfo {
  /** Configured external URL; empty means the UI should fall back to window.location.origin. */
  externalUrl: string;
  /** 是否开放自助注册：signin 页据此显示「注册」入口（公开读，无需认证）。 */
  signupEnabled: boolean;
  /** Signing up confirms the email first: the sign-up page asks only for it. */
  signupVerifiesEmail: boolean;
  /** Passkeys work on this instance: its external URL can be their relying party. */
  passkeySignin: boolean;
  /** Backend product version, injected by the Rust build. */
  version: string;
}

export async function get(): Promise<InstanceInfo> {
  const { data } = await http.get<InstanceInfo>('/instance');
  return data;
}

export interface SignupPolicy {
  signupEnabled: boolean;
  signupRequireApproval: boolean;
  /** Read-only. Without email nobody can prove an address, so signing up always waits for approval. */
  canVerifyEmail?: boolean;
}

/** 读取自助注册策略（`sys.settings.signin.read`）。 */
export async function getSignupPolicy(): Promise<SignupPolicy> {
  const { data } = await http.get<SignupPolicy>('/settings/signup');
  return data;
}

/** 更新自助注册策略（`sys.settings.signin.manage`）。 */
export async function updateSignupPolicy(policy: SignupPolicy): Promise<SignupPolicy> {
  const { data } = await http.put<SignupPolicy>('/settings/signup', policy);
  return data;
}

export type ServiceGraphSource = 'intake' | 'storage';

export interface ServiceGraphSettings {
  /** intake（各进程内存配对+flush）或 storage（单例 worker 从存储重算，跨节点正确）。 */
  source: ServiceGraphSource;
}

/** 读取服务图数据来源模式（`sys.settings.topology.read`）。 */
export async function getServiceGraphSettings(): Promise<ServiceGraphSettings> {
  const { data } = await http.get<ServiceGraphSettings>('/settings/service_graph');
  return data;
}

/** 更新服务图数据来源模式（`sys.settings.topology.manage`）。 */
export async function updateServiceGraphSettings(
  settings: ServiceGraphSettings,
): Promise<ServiceGraphSettings> {
  const { data } = await http.put<ServiceGraphSettings>('/settings/service_graph', settings);
  return data;
}

export interface FederationSettings {
  /** 本集群稳定唯一 id（事件 source/writer）；非空 = 启用联邦，留空 = 关闭。 */
  clusterId: string;
  /** outbox drain → 推送远端周期（秒）。 */
  drainIntervalSecs: number;
  /** 单次推送批量上限。 */
  pushBatchSize: number;
  /** 接收端去重表保留窗口（秒）。 */
  seenEventsTtlSecs: number;
  /** 集群拓扑 gossip 周期（秒）。 */
  gossipIntervalSecs: number;
}

/** 读取跨集群联邦配置（`sys.settings.topology.read`）。 */
export async function getFederationSettings(): Promise<FederationSettings> {
  const { data } = await http.get<FederationSettings>('/settings/federation');
  return data;
}

/** 更新跨集群联邦配置（`sys.settings.topology.manage`）。 */
export async function updateFederationSettings(
  settings: FederationSettings,
): Promise<FederationSettings> {
  const { data } = await http.put<FederationSettings>('/settings/federation', settings);
  return data;
}

export type ClientIpMode = 'peer' | 'header' | 'forwarded_chain';

export interface ClientIpResolverSettings {
  mode: ClientIpMode;
  headerName: string;
  trustedProxyCidrs: string[];
  fallbackToPeer: boolean;
  allowPrivateClientIps: boolean;
  maxChainLength: number;
}

/** 读取部署级 RUM 客户端 IP 识别配置。 */
export async function getClientIpSettings(): Promise<ClientIpResolverSettings> {
  const { data } = await http.get<ClientIpResolverSettings>('/settings/client_ip');
  return data;
}

/** 更新部署级 RUM 客户端 IP 识别配置（`sys.settings.signin.manage`）。 */
export async function updateClientIpSettings(
  settings: ClientIpResolverSettings,
): Promise<ClientIpResolverSettings> {
  const { data } = await http.put<ClientIpResolverSettings>('/settings/client_ip', settings);
  return data;
}

/** 部署级审计记录保留天数，组织审计与平台审计共用；0 表示永久保留。 */
export interface AuditRetentionSettings {
  retentionDays: number;
}

/** 读取审计记录保留天数（`sys.settings.retention.*`）。 */
export async function getAuditRetentionSettings(): Promise<AuditRetentionSettings> {
  const { data } = await http.get<AuditRetentionSettings>('/settings/audit_retention');
  return data;
}

/** 更新审计记录保留天数（`sys.settings.retention.*`）。 */
export async function updateAuditRetentionSettings(
  settings: AuditRetentionSettings,
): Promise<AuditRetentionSettings> {
  const { data } = await http.put<AuditRetentionSettings>('/settings/audit_retention', settings);
  return data;
}

/** 把审计保留期改成 `retention_days` 会多删除的记录：按时间索引数出来的条数，不是估算。 */
export interface AuditRetentionPreview {
  retentionDays: number;
  previousRetentionDays: number;
  rows: number;
  /** 一条都不删时为 `null`。 */
  earliestAt: number | null;
}

/** 调短审计保留期之前，看会多删除哪些记录（`sys.settings.retention.*`）。 */
export async function previewAuditRetention(
  retentionDays: number,
  signal?: AbortSignal,
): Promise<AuditRetentionPreview> {
  const { data } = await http.get<AuditRetentionPreview>('/settings/audit_retention/preview', {
    params: { retentionDays: retentionDays },
    ...(signal ? { signal } : {}),
  });
  return data;
}

/** 数据流没有设置保留天数时保留的天数，1–3650。 */
export interface StreamRetentionSettings {
  retentionDays: number;
}

/** 读取数据流的默认保留天数（`sys.settings.retention.*`）。 */
export async function getStreamRetentionSettings(): Promise<StreamRetentionSettings> {
  const { data } = await http.get<StreamRetentionSettings>('/settings/stream_retention');
  return data;
}

/** 更新数据流的默认保留天数（`sys.settings.retention.*`）。 */
export async function updateStreamRetentionSettings(
  settings: StreamRetentionSettings,
): Promise<StreamRetentionSettings> {
  const { data } = await http.put<StreamRetentionSettings>('/settings/stream_retention', settings);
  return data;
}

/** 一个数据流会多清理掉的量。 */
export interface StreamRetentionImpact {
  orgId: string;
  orgName: string;
  id: string;
  name: string;
  streamType: StreamType;
  rows: number;
  storedBytes: number;
  earliestAt: number | null;
}

/**
 * 把默认保留期调短到 `retention_days` 会多清理掉什么。来自文件目录的元数据，不读数据，所以是
 * 估算（`estimated`），展示时要说明。
 */
export interface StreamRetentionPreview {
  retentionDays: number;
  previousRetentionDays: number;
  estimated: boolean;
  generatedAt: number;
  rows: number;
  storedBytes: number;
  /** 什么也不清理时为 `null`。 */
  earliestAt: number | null;
  /** 受影响的数据流有多少个；`streams` 只列其中最大的一些。 */
  streamCount: number;
  streams: StreamRetentionImpact[];
  /** 读不出目录、没有算进来的数据流有多少个。不是 0 时实际影响可能更大。 */
  unavailableStreams: number;
}

/** 调短默认保留期之前，估算会多清理掉什么（`sys.settings.retention.*`）。 */
export async function previewStreamRetention(
  retentionDays: number,
  signal?: AbortSignal,
): Promise<StreamRetentionPreview> {
  const { data } = await http.get<StreamRetentionPreview>('/settings/stream_retention/preview', {
    params: { retentionDays: retentionDays },
    ...(signal ? { signal } : {}),
  });
  return data;
}

/** 拨测结果保留的天数；0 = 永久保留。 */
export interface SyntheticRetentionSettings {
  retentionDays: number;
}

/** 读取拨测结果的保留天数（`sys.settings.retention.*`）。 */
export async function getSyntheticRetentionSettings(): Promise<SyntheticRetentionSettings> {
  const { data } = await http.get<SyntheticRetentionSettings>('/settings/synthetic_retention');
  return data;
}

/** 更新拨测结果的保留天数（`sys.settings.retention.*`）。 */
export async function updateSyntheticRetentionSettings(
  settings: SyntheticRetentionSettings,
): Promise<SyntheticRetentionSettings> {
  const { data } = await http.put<SyntheticRetentionSettings>(
    '/settings/synthetic_retention',
    settings,
  );
  return data;
}

/** Prometheus 写入的活跃 series 上限：一个组织的，与组织里一个指标的（不超过组织的）。 */
export interface SeriesLimitSettings {
  maxActiveSeriesPerOrg: number;
  maxActiveSeriesPerMetric: number;
}

/** 读取活跃 series 上限（`sys.settings.limits.*`）。 */
/** 接入用量记录保留多久，一个组织每小时最多分别记下多少个服务（`sys.settings.limits.*`）。 */
export interface IntakeUsageSettings {
  retentionDays: number;
  maxServicesPerOrg: number;
}

export async function getIntakeUsageSettings(): Promise<IntakeUsageSettings> {
  const { data } = await http.get<IntakeUsageSettings>('/settings/intake_usage');
  return data;
}

export async function updateIntakeUsageSettings(
  settings: IntakeUsageSettings,
): Promise<IntakeUsageSettings> {
  const { data } = await http.put<IntakeUsageSettings>('/settings/intake_usage', settings);
  return data;
}

export async function getSeriesLimitSettings(): Promise<SeriesLimitSettings> {
  const { data } = await http.get<SeriesLimitSettings>('/settings/series_limits');
  return data;
}

/** 更新活跃 series 上限（`sys.settings.limits.*`）。 */
export async function updateSeriesLimitSettings(
  settings: SeriesLimitSettings,
): Promise<SeriesLimitSettings> {
  const { data } = await http.put<SeriesLimitSettings>('/settings/series_limits', settings);
  return data;
}

/** 分角色集群里，告警规则没设评估延迟时往回挪多少秒（0–3600）。 */
export interface AlertEvaluationSettings {
  evaluationDelaySecs: number;
}

/** 读取告警评估的平台设置（`sys.settings.alerting.*`）。 */
export async function getAlertEvaluationSettings(): Promise<AlertEvaluationSettings> {
  const { data } = await http.get<AlertEvaluationSettings>('/settings/alert_evaluation');
  return data;
}

/** 更新告警评估的平台设置（`sys.settings.alerting.*`）。 */
export async function updateAlertEvaluationSettings(
  settings: AlertEvaluationSettings,
): Promise<AlertEvaluationSettings> {
  const { data } = await http.put<AlertEvaluationSettings>('/settings/alert_evaluation', settings);
  return data;
}

export interface SigninLimitSettings {
  /** Failures before one address is locked out of one account; 0 turns this count off. */
  accountSourceFailures: number;
  /** Failures before one address is locked out of every account; 0 turns this count off. */
  sourceFailures: number;
  /** Failures across addresses before one account is slowed down; 0 turns this count off. */
  accountFailures: number;
  /** While an account is slowed down, how often one password is checked. */
  accountIntervalSecs: number;
  /** How long an address stays locked, and how far back failures count. */
  lockMinutes: number;
}

/** What the sign-in limits see of the request that read the settings. */
export interface ObservedSigninSource {
  address: string | null;
  counted: boolean;
  behindUnconfiguredProxy: boolean;
}

export interface SigninLimitsView extends SigninLimitSettings {
  source: ObservedSigninSource;
}

export async function getSigninLimits(): Promise<SigninLimitsView> {
  const { data } = await http.get<SigninLimitsView>('/settings/signin_limits');
  return data;
}

export async function updateSigninLimits(limits: SigninLimitSettings): Promise<SigninLimitsView> {
  const { data } = await http.put<SigninLimitsView>('/settings/signin_limits', limits);
  return data;
}
