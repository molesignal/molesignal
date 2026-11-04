import type * as ssoApi from '@/api/sso';

import { hostOf } from './oidc';
import type { Oauth2Endpoints, Oauth2Preset, PresetInputs } from './types';

const FEISHU: Record<string, Oauth2Endpoints> = {
  feishu: {
    authorize_url: 'https://accounts.feishu.cn/open-apis/authen/v1/authorize',
    token_url: 'https://open.feishu.cn/open-apis/authen/v2/oauth/token',
    userinfo_url: 'https://open.feishu.cn/open-apis/authen/v1/user_info',
  },
  lark: {
    authorize_url: 'https://accounts.larksuite.com/open-apis/authen/v1/authorize',
    token_url: 'https://open.larksuite.com/open-apis/authen/v2/oauth/token',
    userinfo_url: 'https://open.larksuite.com/open-apis/authen/v1/user_info',
  },
};

function github(host: string): Oauth2Endpoints {
  return {
    authorize_url: `https://${host}/login/oauth/authorize`,
    token_url: `https://${host}/login/oauth/access_token`,
    userinfo_url: host === 'github.com' ? 'https://api.github.com/user' : `https://${host}/api/v3/user`,
  };
}

export const OAUTH2_PRESETS: Oauth2Preset[] = [
  {
    platform: 'github',
    kind: 'oauth2',
    inputs: [
      {
        key: 'deployment',
        label: 'github_deployment',
        initial: 'cloud',
        options: [
          { value: 'cloud', label: 'github_cloud' },
          { value: 'server', label: 'github_server' },
        ],
      },
      {
        key: 'host',
        label: 'github_host',
        placeholder: 'github.example.com',
        shownWhen: { key: 'deployment', value: 'server' },
      },
    ],
    scopes: ['read:user', 'user:email', 'read:org'],
    // The server reads GitHub's id, verified primary email, organizations and teams.
    fieldMapping: { subject: 'id', email: 'email', displayName: 'name', groups: 'groups' },
    fixedMapping: true,
    groups: true,
    hint: 'github',
    endpoints: ({ deployment, host }) => {
      if (deployment !== 'server') return github('github.com');
      const server = hostOf(host);
      return server ? github(server) : null;
    },
    parse: (config) => {
      const match = /^https:\/\/([^/]+)\/login\/oauth\/authorize$/.exec(config.authorizeUrl);
      if (!match?.[1]) return null;
      return match[1] === 'github.com'
        ? { deployment: 'cloud', host: '' }
        : { deployment: 'server', host: match[1] };
    },
  },
  {
    platform: 'feishu',
    kind: 'oauth2',
    inputs: [
      {
        key: 'edition',
        label: 'feishu_edition',
        initial: 'feishu',
        options: [
          { value: 'feishu', label: 'feishu_china' },
          { value: 'lark', label: 'feishu_lark' },
        ],
      },
    ],
    scopes: ['contact:user.email:readonly'],
    // The user arrives wrapped in `data`; the enterprise mailbox comes first when granted.
    fieldMapping: {
      subject: 'data.union_id',
      email: 'data.enterprise_email | data.email',
      displayName: 'data.name',
      groups: 'data.groups',
    },
    groups: false,
    hint: 'feishu',
    endpoints: ({ edition }) => FEISHU[edition ?? 'feishu'] ?? null,
    parse: (config) => {
      const edition = Object.entries(FEISHU).find(
        ([, endpoints]) => endpoints.authorize_url === config.authorizeUrl,
      )?.[0];
      return edition ? { edition } : null;
    },
  },
];

/** The inputs of a saved configuration whose endpoints match the preset's. */
export function parsedOauth2Inputs(
  preset: Oauth2Preset,
  config: ssoApi.Oauth2ProviderConfig,
): PresetInputs | null {
  const inputs = preset.parse(config);
  if (!inputs) return null;
  const endpoints = preset.endpoints(inputs);
  const matches =
    endpoints !== null &&
    endpoints.authorize_url === config.authorizeUrl &&
    endpoints.token_url === config.tokenUrl &&
    endpoints.userinfo_url === config.userinfoUrl;
  return matches ? inputs : null;
}
