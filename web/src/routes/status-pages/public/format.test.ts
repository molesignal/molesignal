import { beforeAll, describe, expect, it } from 'vitest';

import i18n from '@/i18n';

import {
  lasted,
  listNames,
  microsDateTime,
  relativeTime,
  zonedDay,
  zonedDayOfMonth,
} from './format';

const MINUTE = 60 * 1_000_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const now = Date.UTC(2026, 7, 10, 12) * 1_000;

describe('public status formatting', () => {
  beforeAll(async () => {
    await i18n.changeLanguage('en-us');
  });

  it('writes a moment as the datetime of a time element', () => {
    expect(microsDateTime(now)).toBe('2026-08-10T12:00:00.000Z');
  });

  it('says how far a moment is from when the page was generated, in the page language', () => {
    expect(relativeTime(now - 38 * MINUTE, now, 'en-us')).toBe('38 minutes ago');
    expect(relativeTime(now - 20, now, 'en-us')).toBe('1 minute ago');
    expect(relativeTime(now - 5 * HOUR, now, 'en-us')).toBe('5 hours ago');
    expect(relativeTime(now + 2 * DAY, now, 'en-us')).toBe('in 2 days');
    expect(relativeTime(now - 38 * MINUTE, now, 'zh-cn')).toBe('38分钟前');
    expect(relativeTime(now + 2 * DAY, now, 'zh-cn')).toBe('2天后');
  });

  it('says how long something lasted, in days once it ran for two or more', () => {
    const copy = i18n.getFixedT('en-us', 'status-pages');

    expect(lasted(copy, now, now + 20)).toBe('Less than 1 min');
    expect(lasted(copy, now, now + 66 * MINUTE)).toBe('1 hr 6 min');
    expect(lasted(copy, now, now + 3 * HOUR)).toBe('3 hr');
    expect(lasted(copy, now, now + 36 * HOUR)).toBe('36 hr');
    expect(lasted(copy, now, now + 3 * DAY + HOUR)).toBe('3 days');
  });

  it('puts a moment on the day it falls on where the page is, not in UTC', () => {
    const lateEvening = Date.UTC(2026, 7, 8, 23, 30) * 1_000;

    expect(zonedDay(lateEvening, 'UTC', 'en-us')).toEqual({ key: '2026-08-08', label: 'August 8, 2026' });
    expect(zonedDay(lateEvening, 'Asia/Shanghai', 'en-us')).toEqual({
      key: '2026-08-09',
      label: 'August 9, 2026',
    });
    expect(zonedDay(lateEvening, 'Asia/Shanghai', 'zh-cn').label).toBe('2026年8月9日');
  });

  it('names several things as one would say them', () => {
    expect(listNames(['API'], 'en-us')).toBe('API');
    expect(listNames(['API', 'Dashboard'], 'en-us')).toBe('API and Dashboard');
    expect(listNames(['API', 'Dashboard', 'Billing'], 'en-us')).toBe('API, Dashboard, and Billing');
    // Chinese lists with the enumeration comma, which keeps Latin names apart as well.
    expect(listNames(['接口', '控制台'], 'zh-cn')).toBe('接口、控制台');
    expect(listNames(['Public API', 'Web dashboard'], 'zh-cn')).toBe('Public API、Web dashboard');
  });

  it('gives the day of the month as a bare number in the page time zone', () => {
    const lateEvening = Date.UTC(2026, 7, 8, 23, 30) * 1_000;

    expect(zonedDayOfMonth(lateEvening, 'UTC')).toBe('8');
    expect(zonedDayOfMonth(lateEvening, 'Asia/Shanghai')).toBe('9');
  });
});
