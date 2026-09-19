import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Check, Copy, Info, RefreshCw, ShieldCheck, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as statusPagesApi from '@/api/statusPages';
import { writeClipboardText } from '@/lib/clipboard';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import { FormField, FormInput } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { toast } from '@/shell/ui/sonner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { useStatusPageWorkspace } from '../Layout';
import { SettingsCard } from './SettingsSection';

export function CustomDomainSettings() {
  const { t } = useTranslation('status-pages');
  const queryClient = useQueryClient();
  const { pageId, domain, domainCapability, snapshot } = useStatusPageWorkspace();
  const editable = snapshot.page.lifecycle === 'active';
  const domainAvailable = domainCapability?.available === true;
  const [hostname, setHostname] = React.useState(domain?.config.hostname ?? '');
  React.useEffect(() => setHostname(domain?.config.hostname ?? ''), [domain]);
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['status-pages', pageId, 'domain'] });
  const mutation = useMutation({
    mutationFn: async (action: 'configure' | 'verify' | 'retry' | 'remove') => {
      if (action === 'configure') return statusPagesApi.configureDomain(pageId, hostname.trim());
      if (action === 'verify') return statusPagesApi.verifyDomain(pageId);
      if (action === 'retry') return statusPagesApi.retryDomain(pageId);
      await statusPagesApi.removeDomain(pageId);
      return null;
    },
    onSuccess: async (_, action) => {
      toast.success(
        t(action === 'remove' ? 'toast.domain_removed' : 'toast.domain_updated'),
      );
      await refresh();
    },
    onError: (error: unknown, action) => {
      const apiError = toApiError(error);
      toast.error(
        action === 'configure' && apiError.status === 503
          ? t('settings.domain.unavailable_title')
          : apiError.message,
      );
    },
  });
  const config = domain?.config;

  return (
    <SettingsCard title={t('settings.domain.title')} description={t('settings.domain.description')}>
      {domainCapability?.available === false && (
        <div className="flex gap-3 rounded-lg bg-yellow-dim px-4 py-3 text-yellow-soft">
          <Info aria-hidden className="mt-0.5 h-4 w-4 shrink-0" />
          <div className="min-w-0">
            <p className="text-sm font-strong">{t('settings.domain.unavailable_title')}</p>
            <p className="mt-1 text-xs leading-5">
              {t('settings.domain.unavailable_description')}
            </p>
            <code className="mt-2 block overflow-x-auto rounded-md bg-bg-1 px-3 py-2 text-xs text-tx-1">
              <span className="whitespace-pre">{`[http]
external_url = "https://molesignal.example.com"

[http.tls]
enabled = true
account_email = "ops@example.com"`}</span>
            </code>
          </div>
        </div>
      )}
      <div className="flex items-end gap-2">
        <FormField label={t('fields.custom_domain')} className="min-w-0 flex-1">
          <FormInput
            value={hostname}
            placeholder="status.example.com"
            disabled={!editable || !domainAvailable || mutation.isPending}
            onChange={(event) => setHostname(event.currentTarget.value.toLowerCase())}
          />
        </FormField>
        <ChromeButton
          variant="primary"
          disabled={!editable || !domainAvailable || !hostname.trim() || mutation.isPending}
          onClick={() => mutation.mutate('configure')}
        >
          {domain ? t('actions.update_domain') : t('actions.configure_domain')}
        </ChromeButton>
      </div>

      {domain && config && (
        <>
          <div className="grid gap-3 rounded-lg border border-bd-0 bg-bg-2 p-4 sm:grid-cols-3">
            <DomainMetric label={t('fields.domain_state')}>
              <Pill tone={config.state === 'active' ? 'green' : config.state === 'failed' || config.state === 'degraded' ? 'red' : 'yellow'}>
                {t(`domain_state.${config.state}`)}
              </Pill>
            </DomainMetric>
            <DomainMetric label={t('fields.tls')}>
              <span className="inline-flex items-center gap-1.5 text-sm text-tx-1">
                <ShieldCheck
                  className={cn(
                    'h-4 w-4',
                    config.cert_not_after ? 'text-green-soft' : 'text-tx-3',
                  )}
                />
                {config.cert_not_after
                  ? t('settings.domain.valid_until', { date: formatMicrosActive(config.cert_not_after) })
                  : t('settings.domain.pending_certificate')}
              </span>
            </DomainMetric>
            <DomainMetric label={t('fields.last_checked')}>
              <span className="text-sm tabular-nums text-tx-2">
                {config.last_checked_at ? formatMicrosActive(config.last_checked_at) : t('values.never')}
              </span>
            </DomainMetric>
          </div>

          <div className="space-y-2">
            <DnsRecord label={t('settings.domain.txt_name')} value={domain.txt_name} />
            <DnsRecord label={t('settings.domain.txt_value')} value={domain.txt_value} />
            <DnsRecord label={t('settings.domain.routing_target')} value={domain.routing_target} />
          </div>
          {config.last_error && (
            <p role="alert" className="rounded-md bg-red-dim px-3 py-2 text-xs leading-5 text-red-soft">
              {config.last_error}
            </p>
          )}
          <div className="flex flex-wrap justify-end gap-2 border-t border-bd-0 pt-4">
            <ChromeButton
              disabled={!editable || !domainAvailable || mutation.isPending}
              onClick={() => mutation.mutate('verify')}
            >
              <RefreshCw className="h-3.5 w-3.5" />
              {t('actions.verify_domain')}
            </ChromeButton>
            {(config.state === 'failed' || config.state === 'degraded') && (
              <ChromeButton
                disabled={!editable || !domainAvailable || mutation.isPending}
                onClick={() => mutation.mutate('retry')}
              >
                {t('actions.retry')}
              </ChromeButton>
            )}
            <ChromeButton
              disabled={!editable || mutation.isPending}
              className="enabled:hover:text-red-soft"
              onClick={() => mutation.mutate('remove')}
            >
              <Trash2 className="h-3.5 w-3.5" />
              {t('actions.remove_domain')}
            </ChromeButton>
          </div>
        </>
      )}
    </SettingsCard>
  );
}

function DomainMetric({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="mb-1 text-xs text-tx-3">{label}</div>
      {children}
    </div>
  );
}

function DnsRecord({ label, value }: { label: string; value: string }) {
  const { t } = useTranslation('status-pages');
  const [copied, setCopied] = React.useState(false);
  return (
    <div className="flex min-w-0 items-center gap-3 rounded-md border border-bd-0 px-3 py-2">
      <span className="w-28 shrink-0 text-xs text-tx-3">{label}</span>
      <code className="min-w-0 flex-1 truncate text-xs text-tx-1">{value}</code>
      <Tooltip>
        <TooltipTrigger asChild>
          <IconButton
            aria-label={copied ? t('actions.copied') : t('actions.copy')}
            onClick={() => {
              void writeClipboardText(value).then(() => {
                setCopied(true);
                window.setTimeout(() => setCopied(false), 1_500);
              });
            }}
          >
            {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
          </IconButton>
        </TooltipTrigger>
        <TooltipContent>{copied ? t('actions.copied') : t('actions.copy')}</TooltipContent>
      </Tooltip>
    </div>
  );
}
