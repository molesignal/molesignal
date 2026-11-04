import { SCRUB_KINDS } from '@/api/rum';
import type { RumApplication, RumApplicationSettings, RumPlatform, RumSdkSettings, ScrubKind } from '@/api/rum';

import { sameFilters, validFilters } from './retention';
import { sameSegments, validSegments } from './segments';

/** Longest identifier an application is given, as the server keeps it. */
export const MAX_APPLICATION_ID = 64;

const ID_PATTERN = /^[A-Za-z0-9._:-]{1,64}$/;

/**
 * The identifier an application gets from its name when none is typed: lower-case letters and
 * digits joined by single dashes. The server makes the same one; this is what the form shows
 * before it is asked.
 */
export function applicationIdFromName(name: string): string {
  const id = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, MAX_APPLICATION_ID)
    .replace(/-+$/g, '');

  return id || 'app';
}

/** Whether a typed identifier is one SDKs can be configured with. */
export function isApplicationId(value: string): boolean {
  return ID_PATTERN.test(value.trim());
}

/** The most new issues an application can be allowed to open in an hour, as the server keeps it. */
export const MAX_ISSUES_PER_HOUR = 10_000;

/** The most sessions an application can be set to keep: all of them. */
export const MAX_SESSION_SAMPLE_RATE = 100;

export const DEFAULT_SETTINGS: RumApplicationSettings = {
  collectIp: true,
  collectGeo: true,
  defaultPrivacy: 'mask',
  issuesPerHour: 100,
  scrub: [...SCRUB_KINDS],
  sessionSampleRate: MAX_SESSION_SAMPLE_RATE,
  retentionFilters: [],
  keepErrors: true,
  sdk: {},
  segments: [],
};

/** The kinds with one of them turned on or off, in the order they are shown. */
export function withScrub(kinds: readonly ScrubKind[], kind: ScrubKind, on: boolean): ScrubKind[] {
  return SCRUB_KINDS.filter((candidate) => (candidate === kind ? on : kinds.includes(candidate)));
}

/** Whether two sets of what an SDK goes by say the same. */
function sameSdkSettings(left: RumSdkSettings | undefined, right: RumSdkSettings | undefined): boolean {
  const of = (settings: RumSdkSettings | undefined) =>
    JSON.stringify([
      settings?.sessionSampleRate ?? null,
      settings?.sessionReplaySampleRate ?? null,
      settings?.defaultPrivacyLevel ?? null,
      settings?.trackUserInteractions ?? null,
      settings?.forwardConsoleLogs ?? null,
    ]);

  return of(left) === of(right);
}

/** A share an SDK is told is a whole number of percent, or not said at all. */
const validShare = (share: number | undefined) => share === undefined || (Number.isInteger(share) && share >= 0 && share <= 100);

export function sameSettings(left: RumApplicationSettings, right: RumApplicationSettings): boolean {
  return (
    left.collectIp === right.collectIp &&
    left.collectGeo === right.collectGeo &&
    left.defaultPrivacy === right.defaultPrivacy &&
    left.issuesPerHour === right.issuesPerHour &&
    left.sessionSampleRate === right.sessionSampleRate &&
    left.keepErrors === right.keepErrors &&
    sameSdkSettings(left.sdk, right.sdk) &&
    sameFilters(left.retentionFilters, right.retentionFilters) &&
    sameSegments(left.segments ?? [], right.segments ?? []) &&
    SCRUB_KINDS.every((kind) => left.scrub.includes(kind) === right.scrub.includes(kind))
  );
}

/**
 * Whether settings are ones the server takes: the issues an hour are a whole number from 1 to the
 * most, the share of sessions kept a whole number of percent, and the retention filters are named
 * apart, each with a share and with values to compare in every condition.
 */
export function validSettings(settings: RumApplicationSettings): boolean {
  return (
    Number.isInteger(settings.issuesPerHour) &&
    settings.issuesPerHour >= 1 &&
    settings.issuesPerHour <= MAX_ISSUES_PER_HOUR &&
    Number.isInteger(settings.sessionSampleRate) &&
    settings.sessionSampleRate >= 0 &&
    settings.sessionSampleRate <= MAX_SESSION_SAMPLE_RATE &&
    validShare(settings.sdk?.sessionSampleRate) &&
    validShare(settings.sdk?.sessionReplaySampleRate) &&
    validFilters(settings.retentionFilters) &&
    validSegments(settings.segments ?? [])
  );
}

/** The platforms that have an SDK snippet of their own in the setup guide; the others read the docs. */
export const GUIDED_PLATFORMS: readonly RumPlatform[] = ['browser', 'flutter', 'android', 'ios'];

/** The setup guide source of a platform, in the data source pages. */
export function guideSourceOf(platform: RumPlatform): string | null {
  switch (platform) {
    case 'browser':
      return 'rum';
    case 'flutter':
      return 'rum-flutter';
    case 'android':
      return 'rum-android';
    case 'ios':
      return 'rum-ios';
    default:
      return null;
  }
}

/** The applications by name, so a list does not reorder when one is changed. */
export function byName(applications: readonly RumApplication[]): RumApplication[] {
  return [...applications].sort((left, right) => left.name.localeCompare(right.name) || left.id.localeCompare(right.id));
}
