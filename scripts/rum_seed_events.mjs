// Browser sessions as the RUM event contract describes them (contracts/rum/events/v1.schema.json),
// for the seed scripts: the events go to POST /rum/events, the recording to POST /rum/replay.
//
// The server derives the session, the country and the network address of a visitor, and reads the
// browser, the operating system and the device from the User-Agent of the request, so the events
// carry none of them: each session is sent with the User-Agent of one of CLIENTS. Its address goes
// in X-Forwarded-For, which the server uses only where the instance trusts that header.

import { createHash } from 'node:crypto';

import { buildRrwebReplay } from './rum_replay_fixture.mjs';

const WEBKIT = 'AppleWebKit/537.36 (KHTML, like Gecko)';
const DESKTOP = { width: 1440, height: 900 };

/** The browsers the sessions are made to come from. */
export const CLIENTS = [
  {
    name: 'Chrome on macOS',
    userAgent: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) ${WEBKIT} Chrome/126.0.0.0 Safari/537.36`,
    viewport: DESKTOP,
  },
  {
    name: 'Chrome on Windows',
    userAgent: `Mozilla/5.0 (Windows NT 10.0; Win64; x64) ${WEBKIT} Chrome/126.0.0.0 Safari/537.36`,
    viewport: DESKTOP,
  },
  {
    name: 'Safari on iOS',
    userAgent:
      'Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1',
    viewport: { width: 390, height: 844 },
  },
  {
    name: 'Edge on Windows',
    userAgent: `Mozilla/5.0 (Windows NT 10.0; Win64; x64) ${WEBKIT} Chrome/126.0.0.0 Safari/537.36 Edg/126.0.0.0`,
    viewport: DESKTOP,
  },
  {
    name: 'Firefox on macOS',
    userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:127.0) Gecko/20100101 Firefox/127.0',
    viewport: DESKTOP,
  },
  {
    name: 'Safari on macOS',
    userAgent:
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15',
    viewport: DESKTOP,
  },
];

const VIEW_SPACING_MS = 13_000;
const VIEW_DURATION_MS = 12_500;
const CLICK_AFTER_MS = 5_000;

/** The contract counts every duration in whole nanoseconds. */
const ns = (milliseconds) => Math.round(milliseconds * 1_000_000);

/** A W3C span id (16 lower-case hex digits) made from a seed. */
export const hexId = (seed, length = 16) =>
  createHash('sha256').update(seed).digest('hex').slice(0, length);

/** What the visitor did on the last page, by the frustration it shows (none for a plain click). */
const INTERACTIONS = {
  click: { name: 'Continue checkout', selector: '#checkout-primary-action' },
  rage_click: { name: 'Repeated checkout submission', selector: '#checkout-primary-action' },
  dead_click: { name: 'Unresponsive coupon control', selector: '#coupon-toggle' },
};

/**
 * The events of one session, view by view.
 *
 * `metrics(page, index)` gives the load of a page in milliseconds (`lcp`, `inp`, `ttfb`) and its
 * layout shift (`cls`). `request` is the call the visitor's click made on the last page,
 * `{ method, path, status, durationMs, trace?: { traceId, spanId } }`; `errors` are the exceptions
 * thrown there, `{ type, message, fingerprint, frames: [{ file, function, line, column }] }`.
 */
export function sessionEvents(spec) {
  const {
    application,
    service,
    env,
    version,
    origin,
    sessionId,
    userId,
    startedMs,
    client,
    journey,
    metrics,
    request,
    errors = [],
    interaction = 'click',
    longTaskMs,
    vital,
    replay = false,
  } = spec;

  const common = {
    source: 'browser',
    application: { id: application },
    service,
    env,
    version,
    session: { id: sessionId, type: 'user', start: startedMs, has_replay: replay },
    usr: { id: userId },
    display: { viewport: client.viewport },
    sdk: {
      name: 'molesignal-browser-rum',
      version: '0.2.0-alpha.1',
      session_sample_rate: 100,
      replay_sample_rate: 20,
    },
  };

  const lastIndex = journey.length - 1;
  const lastStart = startedMs + lastIndex * VIEW_SPACING_MS;
  const lastView = { id: `${sessionId}.v${lastIndex}`, url: `${origin}${journey[lastIndex]}`, name: journey[lastIndex] };
  const kind = INTERACTIONS[interaction] ?? INTERACTIONS.click;
  const actionId = `${sessionId}.a0`;
  const frustrated = interaction !== 'click';

  const views = journey.map((page, index) => {
    const measured = metrics(page, index);
    const last = index === lastIndex;

    return {
      ...common,
      type: 'view',
      date: startedMs + index * VIEW_SPACING_MS,
      view: {
        id: `${sessionId}.v${index}`,
        url: `${origin}${page}`,
        name: page,
        revision: 2,
        time_spent: ns(VIEW_DURATION_MS),
        is_active: false,
        loading_type: 'initial_load',
        loading_time: ns(measured.lcp + 180),
        first_byte: ns(measured.ttfb),
        performance: {
          fcp: { timestamp: ns(measured.lcp * 0.55) },
          lcp: { timestamp: ns(measured.lcp), target_selector: 'img.hero' },
          inp: { duration: ns(measured.inp), timestamp: ns(measured.lcp + 2_000), target_selector: kind.selector },
          cls: { score: measured.cls, timestamp: ns(measured.lcp + 800) },
        },
        action: { count: last ? 1 : 0 },
        error: { count: last ? errors.length : 0 },
        resource: { count: last ? 1 : 0 },
        long_task: { count: last && longTaskMs ? 1 : 0 },
        frustration: { count: last && frustrated ? 1 : 0 },
      },
    };
  });

  const at = (offsetMs) => lastStart + offsetMs;
  const rest = [];

  if (longTaskMs) {
    rest.push({
      ...common,
      type: 'long_task',
      date: at(2_000),
      view: lastView,
      long_task: {
        id: `${sessionId}.l0`,
        duration: ns(longTaskMs),
        entry_type: 'long-animation-frame',
        start_time: ns(2_000),
        blocking_duration: ns(longTaskMs * 0.7),
      },
    });
  }

  if (vital) {
    rest.push({
      ...common,
      type: 'vital',
      date: at(3_000),
      view: lastView,
      vital: { id: `${sessionId}.t0`, type: 'duration', name: vital.name, duration: ns(vital.durationMs) },
    });
  }

  rest.push({
    ...common,
    type: 'action',
    date: at(CLICK_AFTER_MS),
    view: lastView,
    action: {
      id: actionId,
      type: 'click',
      loading_time: ns(request.durationMs),
      target: { name: kind.name, selector: kind.selector, width: 132, height: 40 },
      position: { x: 1_120, y: 610 },
      name_source: 'text_content',
      ...(frustrated ? { frustration: { type: [interaction] } } : {}),
      error: { count: errors.length },
      resource: { count: 1 },
    },
  });

  rest.push({
    ...common,
    type: 'resource',
    date: at(CLICK_AFTER_MS + 300),
    view: lastView,
    action: { id: actionId },
    resource: {
      id: `${sessionId}.r0`,
      type: 'fetch',
      method: request.method,
      url: `${origin}${request.path}`,
      status_code: request.status,
      duration: ns(request.durationMs),
      ...(request.trace ? { trace_id: request.trace.traceId, span_id: request.trace.spanId } : {}),
    },
  });

  errors.forEach((error, index) => {
    const frames = error.frames.map((frame) => ({ ...frame, file: `${origin}/assets/${frame.file}` }));

    rest.push({
      ...common,
      type: 'error',
      date: at(CLICK_AFTER_MS + 400 + index * 100),
      view: lastView,
      action: { id: actionId },
      error: {
        id: `${sessionId}.e${index}`,
        message: error.message,
        source: 'source',
        type: error.type,
        handling: 'unhandled',
        stack: [
          `${error.type}: ${error.message}`,
          ...frames.map((frame) => `    at ${frame.function} (${frame.file}:${frame.line}:${frame.column})`),
        ].join('\n'),
        frames,
        fingerprint: error.fingerprint,
      },
    });
  });

  return [...views, ...rest];
}

/**
 * The application the sessions belong to: an event for an application that does not exist is
 * refused. `transport` is `{ get(path), post(path, body, headers?) }` against `/api/v1`.
 */
export async function registerRumApplication(transport, application) {
  const known = await transport.get('/rum/applications');

  if (known.some((item) => item.id === application.id)) return;

  await transport.post('/rum/applications', { platform: 'browser', ...application });
}

/** Send one session with the headers of its visitor, and its recording when it has one. */
export async function sendRumSession(transport, spec) {
  const events = sessionEvents(spec);
  const headers = {
    'user-agent': spec.client.userAgent,
    ...(spec.ip ? { 'x-forwarded-for': spec.ip } : {}),
  };

  const report = await transport.post('/rum/events', events, headers);

  if (report.rejected?.length) {
    const { index, reason } = report.rejected[0];
    throw new Error(
      `/rum/events refused ${report.rejected.length} of ${events.length} events, the first (${events[index]?.type}): ${reason}`,
    );
  }

  if (spec.replay) {
    await transport.post('/rum/replay', {
      application: spec.application,
      sessionId: spec.sessionId,
      seq: 1,
      events: buildRrwebReplay({
        startMs: spec.startedMs,
        origin: spec.origin,
        journey: spec.journey,
        interaction: spec.interaction,
        viewport: spec.client.viewport,
        pageSpacingMs: VIEW_SPACING_MS,
        clickAfterMs: CLICK_AFTER_MS,
      }),
    });
  }

  return events;
}
