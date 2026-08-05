import { http } from '@/lib/http';

/** Platform billing control-plane settings. Secrets are never returned —
 *  only `*_set` flags indicate whether they are configured. */
export interface BillingSettings {
  enabled: boolean;
  signature_tolerance_secs: number;
  webhook_secret_set: boolean;
  api_key_set: boolean;
  updated_at_micros: number;
}

export interface BillingSettingsUpdate {
  enabled: boolean;
  signature_tolerance_secs: number;
  /** Omit to keep the existing secret; non-empty to replace. */
  webhook_secret?: string;
  api_key?: string;
}

export async function get(): Promise<BillingSettings> {
  const { data } = await http.get<BillingSettings>('/billing/settings');
  return data;
}

export async function update(input: BillingSettingsUpdate): Promise<BillingSettings> {
  const { data } = await http.put<BillingSettings>('/billing/settings', input);
  return data;
}

/** Per-org trial status. `none` = not on a trial (self-hosted or already paid). */
export interface TrialStatus {
  state: 'none' | 'active' | 'expired' | 'converted';
  started_at_micros: number;
  ends_at_micros: number;
  days_remaining: number;
}

export async function getTrial(): Promise<TrialStatus> {
  const { data } = await http.get<TrialStatus>('/billing/trial');
  return data;
}
