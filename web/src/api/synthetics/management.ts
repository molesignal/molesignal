import { http } from '@/lib/http';

import type {
  ActiveMonitorRevision,
  CreateMonitorInput,
  EgressPolicy,
  LocationLifecycle,
  MonitorDetail,
  MonitorRevision,
  ProbeAgent,
  ProbeAgentToken,
  ProbeAgentTokenInstructions,
  ProbeRegisterInstructions,
  ProbeLocation,
  ProbeOutcome,
  SyntheticMonitor,
  SyntheticResult,
  SyntheticResultPage,
  SyntheticSecret,
  UpdateAgentConfigurationInput,
} from './types';

const monitorPath = (monitorId: string) =>
  `/synthetics/monitors/${encodeURIComponent(monitorId)}`;

export async function listMonitors(): Promise<SyntheticMonitor[]> {
  const { data } = await http.get<SyntheticMonitor[]>('/synthetics/monitors');
  return data;
}

export async function getMonitor(monitorId: string): Promise<MonitorDetail> {
  const { data } = await http.get<MonitorDetail>(monitorPath(monitorId));
  return data;
}

export async function createMonitor(
  input: CreateMonitorInput,
): Promise<ActiveMonitorRevision> {
  const { data } = await http.post<ActiveMonitorRevision>('/synthetics/monitors', input);
  return data;
}

export async function createRevision(
  monitorId: string,
  input: CreateMonitorInput,
): Promise<MonitorRevision> {
  const { data } = await http.post<MonitorRevision>(
    `${monitorPath(monitorId)}/revisions`,
    input,
  );
  return data;
}

export async function testRevision(
  monitorId: string,
  revisionId: string,
): Promise<void> {
  await http.post(
    `${monitorPath(monitorId)}/revisions/${encodeURIComponent(revisionId)}/test`,
  );
}

export async function runMonitor(monitorId: string): Promise<void> {
  await http.post(`${monitorPath(monitorId)}/run`);
}

export async function publishRevision(
  monitorId: string,
  revisionId: string,
): Promise<ActiveMonitorRevision> {
  const { data } = await http.post<ActiveMonitorRevision>(
    `${monitorPath(monitorId)}/revisions/${encodeURIComponent(revisionId)}/publish`,
  );
  return data;
}

export async function pauseMonitor(monitorId: string): Promise<SyntheticMonitor> {
  const { data } = await http.post<SyntheticMonitor>(`${monitorPath(monitorId)}/pause`);
  return data;
}

export async function resumeMonitor(monitorId: string): Promise<SyntheticMonitor> {
  const { data } = await http.post<SyntheticMonitor>(`${monitorPath(monitorId)}/resume`);
  return data;
}

export async function archiveMonitor(monitorId: string): Promise<SyntheticMonitor> {
  const { data } = await http.post<SyntheticMonitor>(`${monitorPath(monitorId)}/archive`);
  return data;
}

export async function listResults(
  monitorId: string,
  options: { before_micros?: number; limit?: number } = {},
): Promise<SyntheticResult[]> {
  const { data } = await http.get<SyntheticResult[]>(`${monitorPath(monitorId)}/results`, {
    params: options,
  });
  return data;
}

export async function listResultPage(options: {
  query?: string;
  outcome?: ProbeOutcome;
  location_id?: string;
  page?: number;
  per_page?: number;
} = {}): Promise<SyntheticResultPage> {
  const { data } = await http.get<SyntheticResultPage>('/synthetics/results', {
    params: options,
  });
  return data;
}

export async function getResultArtifact(
  resultId: string,
  artifactId: string,
): Promise<Blob> {
  const { data } = await http.get<Blob>(
    `/synthetics/results/${encodeURIComponent(resultId)}/artifacts/${encodeURIComponent(artifactId)}`,
    { responseType: 'blob' },
  );
  return data;
}

export async function listLocations(): Promise<ProbeLocation[]> {
  const { data } = await http.get<ProbeLocation[]>('/synthetics/locations');
  return data;
}

export async function createLocation(input: {
  name: string;
  code: string;
  description: string;
  egress_policy: EgressPolicy;
}): Promise<ProbeLocation> {
  const { data } = await http.post<ProbeLocation>('/synthetics/locations', input);
  return data;
}

export async function setLocationLifecycle(
  locationId: string,
  lifecycle: LocationLifecycle,
): Promise<ProbeLocation> {
  const { data } = await http.put<ProbeLocation>(
    `/synthetics/locations/${encodeURIComponent(locationId)}/lifecycle`,
    { lifecycle },
  );
  return data;
}

export async function createRegisterToken(
  locationId: string,
  ttlMinutes = 15,
): Promise<ProbeRegisterInstructions> {
  const { data } = await http.post<ProbeRegisterInstructions>(
    `/synthetics/locations/${encodeURIComponent(locationId)}/register-tokens`,
    { ttl_minutes: ttlMinutes },
  );
  return data;
}

export async function listAgents(locationId: string): Promise<ProbeAgent[]> {
  const { data } = await http.get<ProbeAgent[]>(
    `/synthetics/locations/${encodeURIComponent(locationId)}/agents`,
  );
  return data;
}

export async function listAllAgents(): Promise<ProbeAgent[]> {
  const { data } = await http.get<ProbeAgent[]>('/synthetics/agents');
  return data;
}

export async function revokeAgent(agentId: string): Promise<ProbeAgent> {
  const { data } = await http.post<ProbeAgent>(
    `/synthetics/agents/${encodeURIComponent(agentId)}/revoke`,
  );
  return data;
}

export async function updateAgentConfiguration(
  agentId: string,
  input: UpdateAgentConfigurationInput,
): Promise<ProbeAgent> {
  const { data } = await http.put<ProbeAgent>(
    `/synthetics/agents/${encodeURIComponent(agentId)}/configuration`,
    input,
  );
  return data;
}

export async function listAgentTokens(): Promise<ProbeAgentToken[]> {
  const { data } = await http.get<ProbeAgentToken[]>('/synthetics/agent-tokens');
  return data;
}

export async function createAgentToken(input: {
  name: string;
  location_id: string;
  expires_in_days: number;
}): Promise<ProbeAgentTokenInstructions> {
  const { data } = await http.post<ProbeAgentTokenInstructions>(
    '/synthetics/agent-tokens',
    input,
  );
  return data;
}

export async function rotateAgentToken(
  tokenId: string,
  expiresInDays: number,
): Promise<ProbeAgentTokenInstructions> {
  const { data } = await http.post<ProbeAgentTokenInstructions>(
    `/synthetics/agent-tokens/${encodeURIComponent(tokenId)}/rotate`,
    { expires_in_days: expiresInDays },
  );
  return data;
}

export async function disableAgentToken(tokenId: string): Promise<ProbeAgentToken> {
  const { data } = await http.post<ProbeAgentToken>(
    `/synthetics/agent-tokens/${encodeURIComponent(tokenId)}/disable`,
  );
  return data;
}

export async function listSecrets(): Promise<SyntheticSecret[]> {
  const { data } = await http.get<SyntheticSecret[]>('/synthetics/secrets');
  return data;
}

export async function createSecret(input: {
  name: string;
  description: string;
  value: string;
}): Promise<SyntheticSecret> {
  const { data } = await http.post<SyntheticSecret>('/synthetics/secrets', input);
  return data;
}

export async function rotateSecret(secretId: string, value: string): Promise<SyntheticSecret> {
  const { data } = await http.post<SyntheticSecret>(
    `/synthetics/secrets/${encodeURIComponent(secretId)}/rotate`,
    { value },
  );
  return data;
}
