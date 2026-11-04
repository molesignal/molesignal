import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ExternalLink } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as imApi from '@/api/agent/im';
import type { ImBot } from '@/api/agent/im';
import * as teamsApi from '@/api/teams';
import { writeClipboardText } from '@/lib/clipboard';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { isProductPathDenied, useProductAccess } from '@/product/access';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import {
  PERSONAL,
  botState,
  imDocumentationUrl,
  stateTone,
  teamChoice,
  teamOfChoice,
  type ImBotState,
} from './model';
import { AgentSettingsSection } from '../Surfaces';

const IM_KEY = ['agent', 'im', 'bots'] as const;
const CONNECTORS_PATH = '/settings/notify/connectors';
const HEADER = 'whitespace-nowrap';

const TONE: Record<ReturnType<typeof stateTone>, string> = {
  good: 'text-[var(--green-ink)]',
  bad: 'text-[var(--red-ink)]',
  muted: 'text-tx-2',
};

function StateWord({ state }: { state: ImBotState }) {
  const { t } = useTranslation('agent');

  const word =
    state.kind === 'missing'
      ? t('settings.im.state.missing', {
          fields: state.keys
            .map((key) => t(`settings.im.fields.${key}`, { defaultValue: key }))
            .join(t('settings.im.list_separator')),
        })
      : t(`settings.im.state.${state.kind}`);
  const detail = 'detail' in state ? state.detail : null;

  return (
    <div className="min-w-0 py-1" data-im-state={state.kind}>
      <div className={cn('text-xs', TONE[stateTone(state)])}>{word}</div>
      {detail && (
        <div className="mt-0.5 truncate text-xs text-tx-2" title={detail}>
          {detail}
        </div>
      )}
    </div>
  );
}

/** Where Microsoft Teams posts messages, to enter as the Azure Bot's messaging endpoint. */
function CallbackLine({ bot }: { bot: ImBot }) {
  const { t } = useTranslation('agent');
  const [copied, setCopied] = React.useState(false);
  const url = bot.callbackUrl;

  if (!url) {
    return <div className="mt-0.5 text-xs text-[var(--red-ink)]">{t('settings.im.callback_needs_https')}</div>;
  }

  return (
    <div className="mt-0.5 flex min-w-0 items-center gap-1 text-xs text-tx-2" data-im-callback>
      <span className="shrink-0">{t('settings.im.callback')}</span>
      <span className="min-w-0 truncate text-tx-1" title={url}>
        {url}
      </span>
      <CopyIconButton
        label={t('settings.im.copy_callback')}
        copied={copied}
        copiedLabel={t('settings.im.copied')}
        className="h-6 w-6"
        iconClassName="h-3.5 w-3.5"
        onClick={() => void writeClipboardText(url).then(() => setCopied(true))}
      />
    </div>
  );
}

/**
 * The connectors Mole Agent answers in: Feishu, Slack, DingTalk, WeCom, Google
 * Chat and Microsoft Teams. One server holds each one's long connection or
 * Pub/Sub subscription; Teams posts to a public callback instead. The
 * connector's credentials stay in the notification settings.
 */
export function ImPanel() {
  const { t, i18n } = useTranslation('agent');
  const queryClient = useQueryClient();
  const access = useProductAccess();
  const manageAccess = useActionAccess({ permission: 'agent.im.manage' });
  const bots = useQuery({ queryKey: IM_KEY, queryFn: imApi.listBots, retry: false });
  const teams = useQuery({
    queryKey: ['teams'],
    queryFn: teamsApi.list,
    retry: false,
    staleTime: 5 * 60_000,
  });

  const save = useMutation({
    mutationFn: ({ bot, enabled, team }: { bot: ImBot; enabled: boolean; team: string | null }) =>
      imApi.putBot(bot.connectorId, { enabled, defaultTeamId: team }),
    onSuccess: (saved) =>
      toast.success(
        t(saved.enabled ? 'settings.im.turned_on' : 'settings.im.saved', {
          name: saved.connectorName,
        }),
      ),
    onError: (error) => toast.error(toApiError(error).message),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: IM_KEY }),
  });

  const teamOptions = [
    { value: PERSONAL, label: t('settings.im.team_personal') },
    ...(teams.data ?? []).map((team) => ({ value: team.id, label: team.name })),
  ];

  const guide = (
    <Button asChild variant="ghost" size="sm">
      <a href={imDocumentationUrl(i18n.language)} target="_blank" rel="noreferrer">
        <ExternalLink />
        {t('settings.im.guide')}
      </a>
    </Button>
  );

  const rows = bots.data?.bots ?? [];

  return (
    <AgentSettingsSection
      title={t('settings.tabs.im')}
      description={t('settings.im.description')}
      action={guide}
    >
      {bots.data && !bots.data.externalUrl && (
        <p role="status" className="mb-3 rounded-md bg-bg-2 px-3 py-2 text-xs text-tx-2">
          {t('settings.im.no_external_url')}
        </p>
      )}
      {bots.isPending ? (
        <ProductState variant="loading" />
      ) : bots.isError ? (
        <ProductState variant="error" error={bots.error} />
      ) : rows.length === 0 ? (
        <ProductState
          variant="empty"
          title={t('settings.im.none')}
          description={t('settings.im.none_hint')}
          action={
            isProductPathDenied(CONNECTORS_PATH, access) ? undefined : (
              <Button asChild size="sm">
                <Link to={CONNECTORS_PATH}>{t('settings.im.add_connector')}</Link>
              </Button>
            )
          }
        />
      ) : (
        <DataTable
          rows={rows}
          rowKey={(row) => row.connectorId}
          emptyLabel={t('settings.im.none')}
          columns={[
            {
              key: 'connector',
              header: t('settings.im.columns.connector'),
              className: 'w-full max-w-0',
              headerClassName: HEADER,
              cell: (row) => (
                <div data-im-bot={row.connectorId} className="min-w-0 py-1">
                  <div className="truncate text-tx-0">{row.connectorName}</div>
                  <div className="mt-0.5 truncate text-xs text-tx-2">
                    {row.platform ? t(`settings.im.platforms.${row.platform}`) : row.connectorType}
                    {row.lastMessageAt &&
                      ` · ${t('settings.im.last_message', {
                        at: formatMicrosActive(row.lastMessageAt, false),
                      })}`}
                  </div>
                  {row.platform === 'teams' && <CallbackLine bot={row} />}
                </div>
              ),
            },
            {
              key: 'state',
              header: t('settings.im.columns.state'),
              width: 240,
              headerClassName: HEADER,
              cell: (row) => <StateWord state={botState(row)} />,
            },
            {
              key: 'team',
              header: t('settings.im.columns.team'),
              width: 220,
              headerClassName: HEADER,
              cell: (row) => (
                <FittedSelect
                  value={teamChoice(row)}
                  maxWidth={220}
                  // A table gives a column only what its content cannot do without: this one keeps its label.
                  className="min-w-[168px]"
                  options={teamOptions}
                  aria-label={t('settings.im.team_label', { name: row.connectorName })}
                  disabled={manageAccess.disabled || save.isPending || row.missing.length > 0}
                  onValueChange={(choice) =>
                    save.mutate({ bot: row, enabled: row.enabled, team: teamOfChoice(choice) })
                  }
                />
              ),
            },
            {
              key: 'enabled',
              header: t('settings.im.columns.enabled'),
              width: 72,
              headerClassName: HEADER,
              cell: (row) => (
                <Switch
                  checked={row.enabled}
                  disabled={manageAccess.disabled || save.isPending || (!row.enabled && row.missing.length > 0)}
                  aria-label={t('settings.im.toggle', { name: row.connectorName })}
                  onCheckedChange={(enabled) =>
                    save.mutate({ bot: row, enabled, team: row.defaultTeamId })
                  }
                />
              ),
            },
          ]}
        />
      )}
    </AgentSettingsSection>
  );
}
