import { Check, Clipboard, Link2, Unplug } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { InboundMcpOAuthConnection } from '@/api/agent/inboundMcp';
import { ProductState } from '@/product/states';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import { Input } from '@/shell/ui/input';
import { Label } from '@/shell/ui/label';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

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

export function EndpointField({
  label,
  value,
}: {
  label: string;
  value: string;
}) {
  return (
    <Field label={label}>
      <div className="flex min-w-0 items-center gap-2 rounded-md border border-bd-0 bg-bg-2 px-3 py-2">
        <code className="min-w-0 flex-1 truncate text-xs text-tx-1">
          {value}
        </code>
        <CopyButton value={value} />
      </div>
    </Field>
  );
}

export function CopyButton({ value }: { value: string }) {
  const { t } = useTranslation('agent');
  const [copied, setCopied] = React.useState(false);
  const label = t(
    copied ? 'settings.inbound_mcp.copied' : 'settings.inbound_mcp.copy',
  );
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          size="icon"
          variant="ghost"
          aria-label={label}
          onClick={async () => {
            await navigator.clipboard.writeText(value);
            setCopied(true);
            window.setTimeout(() => setCopied(false), 1_500);
          }}
        >
          {copied ? <Check /> : <Clipboard />}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function formatTimestamp(micros: number): string {
  if (!Number.isFinite(micros)) return '—';
  return new Date(micros / 1_000).toLocaleString();
}
