import { Link2, MessageSquareCode, Unplug } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { InboundMcpOAuthConnection } from '@/api/agent/inboundMcp';
import { writeClipboardText } from '@/lib/clipboard';
import { ProductState } from '@/product/states';
import { CopyField } from '@/shell/CopyField';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import { Input } from '@/shell/ui/input';
import { Label } from '@/shell/ui/label';

export function OAuthConnections({
  connections,
  loading,
  error,
  revoking,
  onRevoke,
}: {
  connections: InboundMcpOAuthConnection[];
  loading: boolean;
  error: Error | null;
  revoking: boolean;
  onRevoke: (id: string) => void;
}) {
  const { t } = useTranslation('agent');
  return (
    <section className="border-t border-bd-0 py-5">
      <div>
        <div className="flex items-center gap-2">
          <Link2 className="h-4 w-4 text-indigo" />
          <h3 className="font-strong text-tx-0">
            {t('settings.inbound_mcp.oauth.title')}
          </h3>
        </div>
        <p className="mt-1 text-xs leading-5 text-tx-3">
          {t('settings.inbound_mcp.oauth.description')}
        </p>
      </div>
      {loading ? (
        <ProductState variant="loading" compact />
      ) : error ? (
        <ProductState variant="error" compact error={error} />
      ) : connections.length === 0 ? (
        <ProductState
          variant="empty"
          compact
          title={t('settings.inbound_mcp.oauth.empty')}
          description={t('settings.inbound_mcp.oauth.empty_description')}
        />
      ) : (
        <div className="mt-4 divide-y divide-bd-0">
          {connections.map((connection) => (
            <div
              key={connection.family_id}
              className="flex flex-wrap items-center gap-3 py-3"
            >
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-strong text-tx-0">
                    {connection.client_name}
                  </span>
                  <Badge variant={connection.active ? 'accent' : 'outline'}>
                    {t(
                      connection.active ? 'status.enabled' : 'status.expired',
                    )}
                  </Badge>
                </div>
                <div className="mt-1 truncate font-mono text-xs text-tx-3">
                  {connection.client_id} · {connection.scope}
                </div>
                <div className="mt-1 text-xs text-tx-3">
                  {t('settings.inbound_mcp.oauth.authorized_by', {
                    value: connection.user_display_name,
                  })}
                </div>
                <div className="mt-1 text-xs text-tx-3">
                  {t('settings.inbound_mcp.oauth.expires', {
                    value: formatTimestamp(connection.last_expires_at),
                  })}
                </div>
              </div>
              {connection.active && (
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={revoking}
                  onClick={() => onRevoke(connection.family_id)}
                >
                  <Unplug /> {t('settings.inbound_mcp.oauth.revoke')}
                </Button>
              )}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

export function Field({
  label,
  htmlFor,
  children,
}: {
  label: string;
  htmlFor?: string | undefined;
  children: React.ReactNode;
}) {
  return (
    <div>
      <Label htmlFor={htmlFor} className="text-xs text-tx-2">{label}</Label>
      <div className="mt-1.5">{children}</div>
    </div>
  );
}

export function NumberField({
  label,
  value,
  min,
  max,
  onChange,
}: {
  label: string;
  value: string;
  min: number;
  max: number;
  onChange: (value: string) => void;
}) {
  const inputId = React.useId();
  return (
    <Field label={label} htmlFor={inputId}>
      <Input
        id={inputId}
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
    </Field>
  );
}

/**
 * An address to copy out, the way the settings show the organization's ID and
 * slug: a filled field one control high, with the shared copy button inside.
 */
export function EndpointField({
  label,
  value,
}: {
  label: string;
  value: string;
}) {
  const { t } = useTranslation('agent');
  return (
    <Field label={label}>
      <CopyField value={value} copyLabel={t('settings.inbound_mcp.copy')} />
    </Field>
  );
}

function formatTimestamp(micros: number): string {
  if (!Number.isFinite(micros)) return '—';
  return new Date(micros / 1_000).toLocaleString();
}

/** How an agent tool reaches MoleSignal: it starts the CLI over stdio, or calls the endpoint. */
type PromptProtocol = 'stdio' | 'http';

/**
 * A prompt to paste into an agent tool (Claude Code, Cursor and the like): it
 * tells the tool how to add MoleSignal as an MCP server. stdio by default: the
 * tool starts `molesignalctl mcp`, signed in once in the browser; or the
 * Streamable HTTP endpoint with OAuth, where the tool calls MoleSignal itself.
 */
export function ConnectPrompt({ endpoint }: { endpoint: string }) {
  const { t } = useTranslation('agent');
  const [protocol, setProtocol] = React.useState<PromptProtocol>('stdio');
  const [copied, setCopied] = React.useState(false);
  const resetTimer = React.useRef<number | undefined>(undefined);
  // The address MoleSignal is reached at: the endpoint without its MCP path.
  const address = endpoint.replace(/\/api\/v1\/mcp\/?$/, '');
  const prompt =
    protocol === 'stdio'
      ? t('settings.inbound_mcp.prompt.text_stdio', { address })
      : t('settings.inbound_mcp.prompt.text_http', { endpoint });

  React.useEffect(() => () => window.clearTimeout(resetTimer.current), []);

  const copy = async () => {
    try {
      await writeClipboardText(prompt);
      setCopied(true);
      window.clearTimeout(resetTimer.current);
      resetTimer.current = window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // The browser may refuse the clipboard; the text stays there to select.
    }
  };

  return (
    <section data-inbound-mcp-prompt className="border-t border-bd-0 py-5">
      <div className="flex items-start gap-3">
        <MessageSquareCode aria-hidden="true" className="mt-0.5 h-4 w-4 text-indigo" />
        <div className="min-w-0 flex-1">
          <h3 className="font-strong text-tx-0">{t('settings.inbound_mcp.prompt.title')}</h3>
          <p className="mt-1 text-xs leading-5 text-tx-3">
            {t('settings.inbound_mcp.prompt.description')}
          </p>
        </div>
        <ViewTabs<PromptProtocol>
          label={t('settings.inbound_mcp.prompt.protocol')}
          value={protocol}
          onValueChange={setProtocol}
          items={[
            { key: 'stdio', label: t('settings.inbound_mcp.prompt.stdio') },
            { key: 'http', label: t('settings.inbound_mcp.prompt.http') },
          ]}
        />
      </div>
      {/* The copy button sits in the box, the way the settings show the organization's ID. */}
      <div className="relative mt-3 rounded-md bg-[var(--control-surface)]">
        <pre
          data-inbound-mcp-prompt-text={protocol}
          className="whitespace-pre-wrap break-words py-2.5 pl-3 pr-12 font-sans text-xs leading-5 text-tx-1"
        >
          {prompt}
        </pre>
        <CopyIconButton
          onClick={() => void copy()}
          label={t('settings.inbound_mcp.prompt.copy')}
          copied={copied}
          copiedLabel={t('settings.inbound_mcp.prompt.copied')}
          wrapperClassName="absolute right-[2px] top-[2px]"
          className="h-11 w-11 lg:h-8 lg:w-8"
        />
      </div>
    </section>
  );
}
