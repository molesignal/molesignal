import { http } from '@/lib/http';

export type RumPlatform = 'browser' | 'android' | 'ios' | 'flutter' | 'react-native' | 'other';

export const RUM_PLATFORMS: readonly RumPlatform[] = [
  'browser',
  'android',
  'ios',
  'flutter',
  'react-native',
  'other',
];

/** A kind of sensitive text an application takes out of its events before they are stored. */
export type ScrubKind = 'secrets' | 'emails' | 'phones' | 'cards' | 'national_ids' | 'url_params';

/** Every kind, in the order they are shown. */
export const SCRUB_KINDS: readonly ScrubKind[] = ['secrets', 'emails', 'phones', 'cards', 'national_ids', 'url_params'];

/** What a retention filter can look at: facts that stay the same while a session lasts. */
export const RETENTION_FIELDS = [
  'env',
  'service',
  'version',
  'source',
  'session.type',
  'geo.country_iso_code',
  'browser.name',
  'os.name',
  'device.type',
] as const;

export type RetentionField = (typeof RETENTION_FIELDS)[number];

/** `is`: the session has one of the values; `is_not`: it has none of them, or none at all. */
export type RetentionOp = 'is' | 'is_not';

/** One thing a retention filter asks of a session; `*` in a value stands for any text. */
export interface RetentionCondition {
  field: RetentionField;
  op: RetentionOp;
  values: string[];
}

/** A share of the sessions that meet some conditions. The first filter that describes a session decides its share. */
export interface RetentionFilter {
  /** What the rows it kept say it was; made by the server when a new filter has none, and kept when it is renamed. */
  id: string;
  name: string;
  /** The share of the sessions it describes that is kept, in percent. */
  rate: number;
  conditions: RetentionCondition[];
}

/** What a segment can look at: what every row of a session says. */
export const SEGMENT_FIELDS = [
  'geo.country_iso_code',
  'geo.continent_code',
  'geo.city',
  'browser.name',
  'os.name',
  'device.type',
  'usr.id',
  'usr.email',
  'account.id',
  'service',
  'version',
  'source',
] as const;

/** A property of the person the application set, as a segment names one: `usr.extra.plan`. */
export const SEGMENT_USER_PREFIX = 'usr.extra.';

export interface RumSegmentCondition {
  field: string;
  op: RetentionOp;
  values: string[];
}

/** A named group of the sessions of an application, that the analytics pages can be narrowed to. */
export interface RumSegment {
  /** What the address of a page names it by; made by the server when a new one has none, and kept when it is renamed. */
  id: string;
  name: string;
  conditions: RumSegmentCondition[];
}

export interface RumApplicationSettings {
  /** Keep the client IP address with each event. */
  collectIp: boolean;
  /** Look the client IP up in the GeoIP database and keep the place with each event. */
  collectGeo: boolean;
  /** The privacy level the SDK snippet suggests. */
  defaultPrivacy: RumPrivacyLevel;
  /** How many new issues the application opens in an hour; past it, new errors are counted together as the rest. */
  issuesPerHour: number;
  /** The kinds of sensitive text taken out of the addresses, the error text, the names and the requests of an event. */
  scrub: ScrubKind[];
  /** The share of the sessions no filter describes that is kept, in percent: a session is kept or dropped as a whole, replay included. */
  sessionSampleRate: number;
  /** Shares of the sessions that meet some conditions; the first that describes a session decides its share. */
  retentionFilters: RetentionFilter[];
  /** Keep the errors of the sessions that are not kept, so that errors are counted as they happened. */
  keepErrors: boolean;
  /** What an SDK that asks at start goes by; left out, it goes by what its page gave it. */
  sdk: RumSdkSettings;
  /** The groups of sessions the analytics pages can be narrowed to. */
  segments?: RumSegment[];
}

export type RumPrivacyLevel = 'mask' | 'mask-user-input' | 'allow';

/** What the settings of an application decide for an SDK that asks at start. */
export interface RumSdkSettings {
  sessionSampleRate?: number;
  sessionReplaySampleRate?: number;
  defaultPrivacyLevel?: RumPrivacyLevel;
  trackUserInteractions?: boolean;
  forwardConsoleLogs?: Array<'debug' | 'log' | 'info' | 'warn' | 'error'>;
}

export interface RumApplication {
  /** The identifier SDKs are configured with; fixed once created. */
  id: string;
  name: string;
  platform: RumPlatform;
  settings: RumApplicationSettings;
  createdBy: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface CreateRumApplication {
  name: string;
  /** Made from the name when left out. */
  id?: string;
  platform?: RumPlatform;
  /** The switches to set; the others take their default on creation and keep their value on a change. */
  settings?: Partial<RumApplicationSettings>;
}

export interface UpdateRumApplication {
  name?: string;
  platform?: RumPlatform;
  /** The switches to set; the others take their default on creation and keep their value on a change. */
  settings?: Partial<RumApplicationSettings>;
}

export async function listApplications(): Promise<RumApplication[]> {
  const { data } = await http.get<RumApplication[]>('/rum/applications');
  return data;
}

export async function getApplication(id: string): Promise<RumApplication> {
  const { data } = await http.get<RumApplication>(`/rum/applications/${encodeURIComponent(id)}`);
  return data;
}

export async function createApplication(body: CreateRumApplication): Promise<RumApplication> {
  const { data } = await http.post<RumApplication>('/rum/applications', body);
  return data;
}

export async function updateApplication(
  id: string,
  body: UpdateRumApplication,
): Promise<RumApplication> {
  const { data } = await http.patch<RumApplication>(
    `/rum/applications/${encodeURIComponent(id)}`,
    body,
  );
  return data;
}

/** Deletes the application and revokes its client token. What it sent stays until retention ends it. */
export async function deleteApplication(id: string): Promise<void> {
  await http.delete(`/rum/applications/${encodeURIComponent(id)}`);
}
