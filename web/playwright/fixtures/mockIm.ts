/**
 * Mock Mole Agent in IM: six connectors (a connected Feishu bot, a Slack app
 * that still lacks its app-level token, a DingTalk bot that cannot connect, a
 * connected WeCom smart bot, a Google Chat app without its Pub/Sub
 * subscription, and a Teams bot with its callback address) and one-time links
 * (`valid-token` links a Feishu account, others are used or expired) — the
 * way `/api/v1/agent/im` answers. Kept per test.
 */
import type { Express } from 'express';

import type { ImBot } from '../../src/api/agent/im';

const MINUTE = 60_000_000;

function seed(now: number): ImBot[] {
  return [
    {
      connectorId: 'conn-feishu',
      connectorName: 'Ops Feishu',
      connectorType: 'lark_app',
      connectorEnabled: true,
      platform: 'feishu',
      missing: [],
      callbackUrl: null,
      enabled: true,
      defaultTeamId: 'team-platform',
      defaultTeamName: 'Platform',
      status: 'connected',
      statusDetail: 'Mole · node-1',
      statusAt: now - 30 * MINUTE,
      lastMessageAt: now - 5 * MINUTE,
    },
    {
      connectorId: 'conn-slack',
      connectorName: 'Ops Slack',
      connectorType: 'slack_app',
      connectorEnabled: true,
      platform: 'slack',
      missing: ['appToken'],
      callbackUrl: null,
      enabled: false,
      defaultTeamId: null,
      defaultTeamName: null,
      status: 'idle',
      statusDetail: null,
      statusAt: null,
      lastMessageAt: null,
    },
    {
      connectorId: 'conn-dingtalk',
      connectorName: 'Ops DingTalk',
      connectorType: 'dingtalk_app',
      connectorEnabled: true,
      platform: 'dingtalk',
      missing: [],
      callbackUrl: null,
      enabled: true,
      defaultTeamId: null,
      defaultTeamName: null,
      status: 'error',
      statusDetail: 'DingTalk refused the Stream connection: HTTP 401 invalid clientSecret',
      statusAt: now - 2 * MINUTE,
      lastMessageAt: null,
    },
    {
      connectorId: 'conn-wecom',
      connectorName: 'Ops WeCom',
      connectorType: 'wecom_bot',
      connectorEnabled: true,
      platform: 'wecom',
      missing: [],
      callbackUrl: null,
      enabled: true,
      defaultTeamId: null,
      defaultTeamName: null,
      status: 'connected',
      statusDetail: 'aibOps · node-2',
      statusAt: now - 10 * MINUTE,
      lastMessageAt: null,
    },
    {
      connectorId: 'conn-google-chat',
      connectorName: 'Ops Google Chat',
      connectorType: 'google_chat_app',
      connectorEnabled: true,
      platform: 'google_chat',
      missing: ['subscription'],
      callbackUrl: null,
      enabled: false,
      defaultTeamId: null,
      defaultTeamName: null,
      status: 'idle',
      statusDetail: null,
      statusAt: null,
      lastMessageAt: null,
    },
    {
      connectorId: 'conn-teams',
      connectorName: 'Ops Teams',
      connectorType: 'teams_app',
      connectorEnabled: true,
      platform: 'teams',
      missing: [],
      callbackUrl: 'https://molesignal.example.com/api/v1/public/im/teams/conn-teams/messages',
      enabled: true,
      defaultTeamId: null,
      defaultTeamName: null,
      status: 'connected',
      statusDetail: '00000000-0000-0000-0000-00000000abcd · node-1',
      statusAt: now - 20 * MINUTE,
      lastMessageAt: now - 2 * MINUTE,
    },
  ];
}

export function registerImRoutes(app: Express, now: number) {
  const bots = seed(now);
  let linked = false;

  app.get('/api/v1/agent/im/bots', (_req, res) => res.json({ bots, externalUrl: true }));

  app.put('/api/v1/agent/im/bots/:id', (req, res) => {
    const bot = bots.find((item) => item.connectorId === req.params.id);
    if (!bot) return res.status(404).json({ error: 'not_found', message: 'not found' });
    if (req.body.enabled && bot.missing.length > 0) {
      return res
        .status(400)
        .json({ error: 'invalid', message: `the connector needs ${bot.missing.join(', ')} to answer in IM` });
    }
    bot.enabled = Boolean(req.body.enabled);
    bot.defaultTeamId = req.body.defaultTeamId ?? null;
    bot.defaultTeamName =
      bot.defaultTeamId === 'team-platform' ? 'Platform' : bot.defaultTeamId ? 'Payments' : null;
    bot.status = bot.enabled ? bot.status : 'idle';
    return res.json(bot);
  });

  app.delete('/api/v1/agent/im/bots/:id', (_req, res) => res.json({ deleted: true }));

  app.get('/api/v1/agent/im/links/:token', (req, res) => {
    if (req.params.token !== 'valid-token' || linked) {
      return res.status(404).json({ error: 'not_found', message: 'this link was used or has expired; ask in IM again' });
    }
    return res.json({
      platform: 'feishu',
      connectorName: 'Ops Feishu',
      externalIdentity: 'ou_alice',
      displayName: 'Alice',
      expiresAt: now + 15 * MINUTE,
    });
  });

  app.post('/api/v1/agent/im/links/:token', (req, res) => {
    if (req.params.token !== 'valid-token' || linked) {
      return res.status(404).json({ error: 'not_found', message: 'this link was used or has expired; ask in IM again' });
    }
    linked = true;
    return res.json({ linked: true, endpointId: 'endpoint-1' });
  });
}
