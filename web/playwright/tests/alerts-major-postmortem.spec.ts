/**
 * Major incidents and internal postmortems: declaring one gives it roles, a war room, a checklist
 * and updates; a postmortem starts from the template and collects action items.
 */
import type { Page } from '@playwright/test';

import { FROZEN_NOW_ISO, expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1000;

async function capture(page: Page, pattern: RegExp, method: string, reply: (body: Record<string, unknown>) => unknown) {
  const sent: Array<Record<string, unknown>> = [];
  await page.route(pattern, async (route) => {
    if (route.request().method() !== method) return route.fallback();
    const body = (route.request().postDataJSON() ?? {}) as Record<string, unknown>;
    sent.push(body);
    return route.fulfill({ json: reply(body) });
  });
  return sent;
}

test('declaring a major incident gives it roles, a war room, a checklist and updates', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const response = {
    incidentId: 'inc-2292',
    declaredAt: NOW,
    declaredBy: 'u1',
    roles: { commander: 'u1' },
    warRoom: { platform: 'lark_app', connectorId: 'c-feishu', chatId: 'oc_1', url: 'https://applink.feishu.cn/client/chat/oc_1' },
    meetingUrl: 'https://vc.feishu.cn/j/123456',
    updatedAt: NOW,
  };
  const task = { id: 'task-1', incidentId: 'inc-2292', title: 'Page the payments team', assigneeId: null, done: false, doneBy: null, doneAt: null, position: 0, createdAt: NOW };
  let state: { response: unknown; tasks: unknown[] } = { response: null, tasks: [] };
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-2292\/response$/, (route) => route.fulfill({ json: state }));
  const declared = await capture(page, /\/api\/v1\/alerts\/incidents\/inc-2292\/declare$/, 'POST', () => {
    state = { response, tasks: [task] };
    return state;
  });
  const roles = await capture(page, /\/api\/v1\/alerts\/incidents\/inc-2292\/roles$/, 'PUT', (body) => {
    state = { response: { ...response, roles: { ...response.roles, [String(body.role)]: body.userId } }, tasks: state.tasks };
    return {};
  });
  const done = await capture(page, /\/api\/v1\/alerts\/incidents\/inc-2292\/tasks\/task-1$/, 'PUT', () => {
    state = { response: state.response, tasks: [{ ...task, done: true, doneBy: 'dev' }] };
    return state.tasks[0];
  });
  const updates = await capture(page, /\/api\/v1\/alerts\/incidents\/inc-2292\/status-updates$/, 'POST', () => ({ sent: true }));

  await page.goto('/alerts/incidents/inc-2292');
  const major = page.locator('[data-incident-major]');
  await major.getByRole('button', { name: 'Declare major incident' }).click();
  await expect(page.getByText('Declared a major incident')).toBeVisible();
  expect(declared).toHaveLength(1);

  await expect(major).toContainText('Declared major by Dana Reyes');
  await expect(major.getByRole('link', { name: 'Join the war room (Feishu)' })).toHaveAttribute('href', response.warRoom.url);
  await expect(major.getByRole('link', { name: 'Join the meeting' })).toHaveAttribute('href', response.meetingUrl);

  await major.getByRole('combobox', { name: 'Scribe' }).click();
  await page.getByRole('option', { name: 'Amara Okafor' }).click();
  await expect.poll(() => roles[0]).toEqual({ role: 'scribe', userId: 'u3' });

  await major.getByRole('checkbox', { name: 'Page the payments team' }).click();
  await expect.poll(() => done[0]).toEqual({ done: true });

  await major.getByRole('button', { name: 'Post an update' }).click();
  const dialog = page.getByRole('dialog', { name: 'Post an update' });
  await dialog.getByRole('textbox', { name: 'Impact' }).fill('EU checkout failing for 30% of users');
  await dialog.getByRole('textbox', { name: 'Where things are' }).fill('Rolling back v2026.10.28');
  await dialog.getByRole('button', { name: 'Send' }).click();
  await expect(page.getByText('Update sent')).toBeVisible();
  expect(updates[0]).toEqual({ impact: 'EU checkout failing for 30% of users', current: 'Rolling back v2026.10.28', next: '' });
});

test('a postmortem starts from the template and collects action items', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const postmortem = (body: string) => ({
    id: 'pm-1',
    incidentId: 'inc-2292',
    title: 'Checkout errors',
    status: 'draft',
    body,
    ownerId: 'dev',
    reviewerIds: [],
    createdBy: 'dev',
    createdAt: NOW,
    updatedAt: NOW,
    publishedAt: null,
  });
  let detail: { postmortem: unknown; actions: unknown[] } = { postmortem: null, actions: [] };
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-2292\/postmortem$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: detail }) : route.fallback(),
  );
  const created = await capture(page, /\/api\/v1\/alerts\/incidents\/inc-2292\/postmortem$/, 'POST', (body) => {
    detail = { postmortem: postmortem(String(body.body)), actions: [] };
    return detail;
  });
  const actions = await capture(page, /\/api\/v1\/alerts\/postmortems\/pm-1\/actions$/, 'POST', (body) => {
    const item = {
      id: 'ai-1',
      postmortemId: 'pm-1',
      incidentId: 'inc-2292',
      title: body.title,
      ownerId: null,
      dueAt: null,
      status: 'open',
      ticketId: null,
      ticketKey: '',
      ticketUrl: '',
      createdAt: NOW,
      updatedAt: NOW,
    };
    detail = { ...detail, actions: [item] };
    return item;
  });
  const saved = await capture(page, /\/api\/v1\/alerts\/postmortems\/pm-1$/, 'PUT', (body) => ({ postmortem: { ...postmortem(String(body.body)), status: body.status }, actions: detail.actions }));

  await page.goto('/alerts/incidents/inc-2292');
  await page.locator('[data-incident-postmortem]').getByRole('button', { name: 'Write postmortem' }).click();

  await expect.poll(() => created.length).toBe(1);
  const body = String(created[0]!.body);
  for (const heading of ['## Summary', '## Impact', '## Timeline', '## Root cause', '## What went well', '## Action items']) {
    expect(body).toContain(heading);
  }

  const drawer = page.locator('[data-postmortem-drawer]');
  await expect(drawer.getByRole('textbox', { name: 'Text (Markdown)' })).toHaveValue(body);
  await drawer.getByRole('textbox', { name: 'Add an action item' }).fill('Alert on payments pool saturation');
  await drawer.getByRole('button', { name: 'Add' }).click();
  await expect.poll(() => actions[0]).toEqual({ title: 'Alert on payments pool saturation' });
  await expect(drawer.locator('[data-postmortem-action="ai-1"]')).toContainText('Alert on payments pool saturation');

  await drawer.getByRole('combobox', { name: 'Status' }).click();
  await page.getByRole('option', { name: 'In review' }).click();
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect.poll(() => saved[0]?.status).toBe('in_review');
});

test('major incidents follow the organization settings', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const saved = await capture(page, /\/api\/v1\/alerts\/response\/settings$/, 'PUT', (body) => body);

  await page.goto('/alerts/postmortems');
  await expect(page.getByText('No postmortems yet.')).toBeVisible();
  await page.getByRole('button', { name: 'Major incident settings' }).click();
  const drawer = page.locator('[data-response-settings]');
  await drawer.getByRole('combobox', { name: 'Meeting' }).click();
  await page.getByRole('option', { name: 'A fixed meeting link' }).click();
  await drawer.getByRole('textbox', { name: 'Meeting link' }).fill('https://meet.example.com/war-room');
  await drawer.locator('textarea').fill('Open a status page incident\nPage the service owner');
  await page.getByRole('checkbox', { name: 'Declare automatically' }).click();
  await page.getByRole('button', { name: 'Save' }).click();

  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({
    meeting: 'fixed_url',
    meetingUrl: 'https://meet.example.com/war-room',
    checklist: ['Open a status page incident', 'Page the service owner'],
    auto: { enabled: true, minSeverity: null, conditions: [] },
  });
});

const connector = (id: string, name: string, connectorType: string) => ({
  id,
  organizationId: 'org-1',
  name,
  connectorType: connectorType,
  config: {},
  capabilities: {},
  enabled: true,
  status: 'connected',
  createdAt: NOW,
  updatedAt: NOW,
});

test('war rooms list the connectors that can make group chats, and a Zoom secret is written once', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route(/\/api\/v1\/notify\/connectors$/, (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: [
            connector('c-feishu', 'Feishu alerts', 'lark_app'),
            connector('c-ding', 'DingTalk ops', 'dingtalk_app'),
            connector('c-bot', 'WeCom robot', 'wecom_bot'),
            connector('c-hook', 'Slack hook', 'slack_webhook'),
          ],
        })
      : route.fallback(),
  );
  const saved = await capture(page, /\/api\/v1\/alerts\/response\/settings$/, 'PUT', (body) => ({
    ...body,
    secretsSet: Object.keys((body.secrets as Record<string, string>) ?? {}),
    warRoomTypes: ['lark_app', 'slack_app', 'dingtalk_app', 'wecom_app'],
  }));

  await page.goto('/alerts/postmortems');
  await page.getByRole('button', { name: 'Major incident settings' }).click();
  const drawer = page.locator('[data-response-settings]');

  await drawer.getByRole('combobox', { name: 'Connector for war rooms' }).click();
  await expect(page.getByRole('option', { name: 'Feishu alerts · Feishu' })).toBeVisible();
  await expect(page.getByRole('option', { name: /WeCom robot|Slack hook/ })).toHaveCount(0);
  await page.getByRole('option', { name: 'DingTalk ops · DingTalk' }).click();
  await expect(drawer).toContainText("the app's robot has to be added to the group");

  await drawer.getByRole('combobox', { name: 'Meeting' }).click();
  await page.getByRole('option', { name: 'Start a Zoom meeting' }).click();
  const zoom = drawer.locator('[data-meeting-fields="zoom"]');
  await zoom.getByRole('textbox', { name: 'Account ID' }).fill('acct-1');
  await zoom.getByRole('textbox', { name: 'Client ID' }).fill('client-1');
  await zoom.getByLabel('Client Secret').fill('zoom-secret');
  await zoom.getByRole('textbox', { name: 'Host' }).fill('oncall@example.com');
  await page.getByRole('button', { name: 'Save' }).click();

  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({
    warRoomConnectorId: 'c-ding',
    meeting: 'zoom',
    zoom: { accountId: 'acct-1', clientId: 'client-1', host: 'oncall@example.com' },
    secrets: { 'zoom.clientSecret': 'zoom-secret' },
  });
});

test('a saved meeting secret is not shown again and is kept when left empty', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route(/\/api\/v1\/alerts\/response\/settings$/, (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: {
            warRoomConnectorId: null,
            meeting: 'tencent_meeting',
            meetingUrl: '',
            meetingConnectorId: null,
            tencentMeeting: { appId: '200000001', sdkId: '', secretId: 'AKID', userid: 'dana' },
            zoom: { accountId: '', clientId: '', host: '' },
            googleMeet: { organizer: '' },
            checklist: [],
            stakeholderIds: [],
            auto: { enabled: false, conditions: [], minSeverity: null },
            secretsSet: ['tencentMeeting.secretKey'],
            warRoomTypes: ['lark_app', 'slack_app', 'dingtalk_app', 'wecom_app'],
          },
        })
      : route.fallback(),
  );
  const saved = await capture(page, /\/api\/v1\/alerts\/response\/settings$/, 'PUT', (body) => body);

  await page.goto('/alerts/postmortems');
  await page.getByRole('button', { name: 'Major incident settings' }).click();
  const tencent = page.locator('[data-meeting-fields="tencent_meeting"]');
  await expect(tencent).toContainText('Set; leave empty to keep it');
  await expect(tencent.getByLabel('SecretKey')).toHaveValue('');
  await page.getByRole('button', { name: 'Save' }).click();

  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({ meeting: 'tencent_meeting', secrets: {} });
});

test('a DingTalk war room has no join link and says it was made', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-2292\/response$/, (route) =>
    route.fulfill({
      json: {
        response: {
          incidentId: 'inc-2292',
          declaredAt: NOW,
          declaredBy: 'u1',
          roles: { commander: 'u1' },
          warRoom: { platform: 'dingtalk_app', connectorId: 'c-ding', chatId: 'cid-1', url: '' },
          meetingUrl: 'https://meeting.tencent.com/dm/abc',
          updatedAt: NOW,
        },
        tasks: [],
      },
    }),
  );

  await page.goto('/alerts/incidents/inc-2292');
  const major = page.locator('[data-incident-major]');
  await expect(major.locator('[data-war-room-made]')).toHaveText('War room made (DingTalk)');
  await expect(major.getByRole('link', { name: /war room/i })).toHaveCount(0);
  await expect(major.getByRole('link', { name: 'Join the meeting' })).toHaveAttribute('href', 'https://meeting.tencent.com/dm/abc');
});
