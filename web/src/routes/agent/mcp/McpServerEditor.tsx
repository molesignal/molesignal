import { useMutation, useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as agentApi from '@/api/agent';
import { toApiError } from '@/lib/http';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';

import { inputsFrom, type NamedValueRow, rowsFrom } from './namedValues';
import { NamedValuesEditor } from './NamedValuesEditor';
import { ToggleField } from './ToggleField';
import { scopeChoice, scopeTarget, useScopes } from '../scope/useScopes';

const TRANSPORTS: agentApi.McpTransport[] = ['streamable_http', 'sse', 'stdio'];

const AUTH_TYPES: agentApi.McpAuthType[] = [
  'none',
  'bearer_token',
  'api_key',
  'member_secret',
  'oauth',
  'mtls',
  'internal_service_identity',
];

function splitLines(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

/**
 * Adds or changes an MCP server. An organization server can be limited to the
 * private network, to domains and address ranges, and can skip certificate
 * checks; a team or personal one connects by the organization's network
 * policy for members and always checks certificates.
 */
export function McpServerEditor({
  target,
  onClose,
  onSaved,
}: {
  target: agentApi.McpServer | 'new' | null;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation('agent');
  const scopes = useScopes();
  const existing = target && target !== 'new' ? target : null;
  const [scope, setScope] = React.useState('personal');
  const [name, setName] = React.useState('');
  const [transport, setTransport] = React.useState<agentApi.McpTransport>('streamable_http');
  const [endpointUrl, setEndpointUrl] = React.useState('');
  const [command, setCommand] = React.useState('');
  const [authType, setAuthType] = React.useState<agentApi.McpAuthType>('none');
  const [authHeader, setAuthHeader] = React.useState('');
  const [credential, setCredential] = React.useState('');
  const [memberHeader, setMemberHeader] = React.useState('authorization');
  const [memberBearer, setMemberBearer] = React.useState(true);
  const [memberInstructions, setMemberInstructions] = React.useState('');
  const [headers, setHeaders] = React.useState<NamedValueRow[]>([]);
  const [env, setEnv] = React.useState<NamedValueRow[]>([]);
  const [privateOnly, setPrivateOnly] = React.useState(true);
  const [allowedDomains, setAllowedDomains] = React.useState('');
  const [allowedCidrs, setAllowedCidrs] = React.useState('');
  const [tlsVerify, setTlsVerify] = React.useState(true);
  const [timeoutMs, setTimeoutMs] = React.useState('10000');
  const [maxResponseBytes, setMaxResponseBytes] = React.useState('1048576');
  const [enabled, setEnabled] = React.useState(true);

  React.useEffect(() => {
    setScope(existing ? scopeChoice({ scope: existing.scope.scope, team_id: teamOf(existing) }) : 'personal');
    setName(existing?.name ?? '');
    setTransport(existing?.transport ?? 'streamable_http');
    setEndpointUrl(existing?.endpoint_url ?? '');
    setCommand(existing?.command_template ?? '');
    setAuthType(existing?.auth_type ?? 'none');
    setAuthHeader(existing?.auth_header ?? '');
    setCredential('');
    setMemberHeader(existing?.member_credential?.header ?? 'authorization');
    setMemberBearer(existing?.member_credential?.bearer ?? true);
    setMemberInstructions(existing?.member_credential?.instructions ?? '');
    setHeaders(rowsFrom(existing?.headers));
    setEnv(rowsFrom(existing?.env));
    setPrivateOnly(existing?.private_only ?? true);
    setAllowedDomains((existing?.allowed_domains ?? []).join('\n'));
    setAllowedCidrs((existing?.allowed_cidrs ?? []).join('\n'));
    setTlsVerify(existing?.tls_verify ?? true);
    setTimeoutMs(String(existing?.timeout_ms ?? 10000));
    setMaxResponseBytes(String(existing?.max_response_bytes ?? 1048576));
    setEnabled(existing?.enabled ?? true);
  }, [existing, target]);

  const organization = scope === 'org';
  const remote = transport !== 'stdio';
  const sharedCredential = !['none', 'member_secret'].includes(authType);
  // stdio is the organization's: MoleSignal starts the command, among those the instance allows.
  const stdioCommands = useQuery({
    queryKey: ['agent', 'mcp-stdio-commands'],
    queryFn: agentApi.mcpStdioCommands,
    staleTime: 60_000,
    retry: false,
  }).data ?? [];
  const transports = TRANSPORTS.filter((value) => value !== 'stdio' || organization || existing?.transport === 'stdio');
  React.useEffect(() => {
    if (!organization && transport === 'stdio' && existing?.transport !== 'stdio') {
      setTransport('streamable_http');
    }
  }, [organization, transport, existing]);
  const stdioNote = !organization
    ? { tone: 'warning', text: t('settings.tools.mcp.editor.stdio_org_only') }
    : stdioCommands.length === 0
      ? { tone: 'warning', text: t('settings.tools.mcp.editor.stdio_off') }
      : {
          tone: 'plain',
          text: t('settings.tools.mcp.editor.stdio_note', { commands: stdioCommands.join(', ') }),
        };

  // The scope it is in stays offered when editing, even when it would not be offered to add to.
  const scopeOptions = React.useMemo(() => {
    if (scopes.options.some((option) => option.value === scope)) return scopes.options;
    return existing
      ? [
          ...scopes.options,
          {
            value: scope,
            label: scopes.label({
              scope: existing.scope.scope,
              team_id: teamOf(existing) ?? null,
              owner_id: ownerOf(existing) ?? null,
            }),
          },
        ]
      : scopes.options;
  }, [existing, scope, scopes]);

  const mutation = useMutation({
    mutationFn: () => {
      const target = scopeTarget(scope);
      const input: agentApi.McpServerInput = {
        ...target,
        name,
        transport,
        // A stdio server is not reached over HTTP: its credentials are its environment.
        auth_type: remote ? authType : 'none',
        headers: remote ? inputsFrom(headers) : [],
        env: remote ? [] : inputsFrom(env),
        private_only: remote && organization && privateOnly,
        allowed_domains: remote && organization ? splitLines(allowedDomains) : [],
        allowed_cidrs: remote && organization ? splitLines(allowedCidrs) : [],
        follow_redirects: false,
        tls_verify: !remote || !organization || tlsVerify,
        timeout_ms: Number(timeoutMs),
        max_response_bytes: Number(maxResponseBytes),
        enabled,
        ...(remote && endpointUrl ? { endpoint_url: endpointUrl } : {}),
        ...(!remote && command ? { command_template: command } : {}),
        ...(remote && authHeader && ['api_key', 'internal_service_identity'].includes(authType)
          ? { auth_header: authHeader }
          : {}),
        ...(remote && credential && sharedCredential ? { credential } : {}),
        ...(remote && authType === 'member_secret'
          ? {
              member_credential: {
                header: memberHeader,
                bearer: memberBearer,
                instructions: memberInstructions,
              },
            }
          : {}),
      };
      return existing
        ? agentApi.updateMcpServer(existing.id, input)
        : agentApi.createMcpServer(input);
    },
    onSuccess: async () => {
      toast.success(
        t(existing ? 'settings.tools.mcp.feedback.updated' : 'settings.tools.mcp.feedback.created'),
      );
      await onSaved();
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  if (!target) return null;

  const complete =
    name.trim() &&
    (remote ? endpointUrl.trim() : command.trim()) &&
    (!remote || authType !== 'member_secret' || memberHeader.trim());

  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={t(existing ? 'settings.tools.mcp.editor.edit_title' : 'settings.tools.mcp.editor.create_title')}
      subtitle={t('settings.tools.mcp.editor.description')}
      width={680}
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button onClick={() => mutation.mutate()} disabled={mutation.isPending || !complete}>
            {mutation.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </>
      }
    >
      <FormSection title={t('settings.tools.mcp.editor.basic')}>
        <FormField label={t('settings.tools.mcp.fields.name')} required>
          <FormInput
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={t('settings.tools.mcp.fields.name_placeholder')}
          />
        </FormField>
        <FormField
          label={t('scope.field')}
          hint={t(organization ? 'scope.hint_org' : scope.startsWith('team:') ? 'scope.hint_team' : 'scope.hint_personal')}
        >
          <FormSelect
            value={scope}
            onChange={setScope}
            options={scopeOptions}
            ariaLabel={t('scope.field')}
            className="w-full"
          />
        </FormField>
        <FormField label={t('settings.tools.mcp.fields.transport')} required>
          <FormSelect
            value={transport}
            onChange={(value) => setTransport(value as agentApi.McpTransport)}
            options={transports.map((value) => ({
              value,
              label: t(`settings.tools.mcp.transport.${value}`),
            }))}
            className="w-full"
          />
        </FormField>
        {remote ? (
          <FormField
            label={t('settings.tools.mcp.fields.endpoint')}
            {...(transport === 'sse' ? { hint: t('settings.tools.mcp.fields.endpoint_sse_hint') } : {})}
            required
          >
            <FormInput
              type="url"
              value={endpointUrl}
              onChange={(event) => setEndpointUrl(event.target.value)}
              placeholder={t(
                transport === 'sse'
                  ? 'settings.tools.mcp.fields.endpoint_sse_placeholder'
                  : 'settings.tools.mcp.fields.endpoint_placeholder',
              )}
            />
          </FormField>
        ) : (
          <>
            <FormField label={t('settings.tools.mcp.fields.command')} required>
              <FormInput
                value={command}
                onChange={(event) => setCommand(event.target.value)}
                placeholder="npx -y @modelcontextprotocol/server-github"
                className="font-mono"
              />
            </FormField>
            <div
              role="note"
              data-agent-mcp-stdio-note={stdioNote.tone}
              className={cn(
                'rounded-md p-3 text-xs leading-5',
                stdioNote.tone === 'warning'
                  ? 'bg-[var(--yellow-tint)] text-[var(--yellow-ink)]'
                  : 'bg-bg-2 text-tx-2',
              )}
            >
              {stdioNote.text}
            </div>
          </>
        )}
      </FormSection>

      {remote && (
        <>
          <FormSection
            title={t('settings.tools.mcp.editor.authentication')}
            {...(sharedCredential
              ? {
                  description: existing?.credential_set
                    ? t('settings.tools.mcp.editor.credential_set', {
                        last4: existing.credential_last4 ?? '',
                      })
                    : t('settings.tools.mcp.editor.credential_not_set'),
                }
              : {})}
          >
            <FormField label={t('settings.tools.mcp.fields.auth_type')}>
              <FormSelect
                value={authType}
                onChange={(value) => setAuthType(value as agentApi.McpAuthType)}
                options={AUTH_TYPES.map((value) => ({
                  value,
                  label: t(`settings.tools.mcp.auth.${value}`),
                }))}
                className="w-full"
              />
            </FormField>
            {['api_key', 'internal_service_identity'].includes(authType) && (
              <FormField label={t('settings.tools.mcp.fields.auth_header')}>
                <FormInput
                  value={authHeader}
                  onChange={(event) => setAuthHeader(event.target.value)}
                  placeholder={t('settings.tools.mcp.fields.auth_header_placeholder')}
                />
              </FormField>
            )}
            {sharedCredential && (
              <FormField
                label={t('settings.tools.mcp.fields.credential')}
                hint={t('settings.tools.mcp.fields.credential_hint')}
                required={!existing?.credential_set}
              >
                <FormInput
                  type="password"
                  autoComplete="new-password"
                  value={credential}
                  onChange={(event) => setCredential(event.target.value)}
                  placeholder={t('settings.tools.mcp.fields.credential_placeholder')}
                />
              </FormField>
            )}
            {authType === 'member_secret' && (
              <>
                <p className="text-xs leading-5 text-tx-3">
                  {t('settings.tools.mcp.member.description')}
                </p>
                <FormRow>
                  <FormField label={t('settings.tools.mcp.member.header')} required>
                    <FormInput
                      value={memberHeader}
                      onChange={(event) => setMemberHeader(event.target.value)}
                      placeholder="authorization"
                      className="font-mono"
                    />
                  </FormField>
                  <ToggleField
                    label={t('settings.tools.mcp.member.bearer')}
                    hint={t('settings.tools.mcp.member.bearer_hint')}
                    checked={memberBearer}
                    onChange={setMemberBearer}
                  />
                </FormRow>
                <FormField
                  label={t('settings.tools.mcp.member.instructions')}
                  hint={t('settings.tools.mcp.member.instructions_hint')}
                >
                  <FormTextarea
                    value={memberInstructions}
                    onChange={(event) => setMemberInstructions(event.target.value)}
                    rows={3}
                  />
                </FormField>
              </>
            )}
          </FormSection>

          <FormSection
            title={t('settings.tools.mcp.editor.headers')}
            description={t('settings.tools.mcp.editor.headers_description')}
          >
            <NamedValuesEditor rows={headers} onChange={setHeaders} kind="header" />
          </FormSection>
        </>
      )}

      {!remote && (
        <FormSection
          title={t('settings.tools.mcp.editor.env')}
          description={t('settings.tools.mcp.editor.env_description')}
        >
          <NamedValuesEditor rows={env} onChange={setEnv} kind="env" />
        </FormSection>
      )}

      {remote && (
        <FormSection title={t('settings.tools.mcp.editor.network')}>
          {organization ? (
            <>
              <ToggleField
                label={t('settings.tools.mcp.fields.private_only')}
                hint={t('settings.tools.mcp.fields.private_only_hint')}
                checked={privateOnly}
                onChange={setPrivateOnly}
              />
              <FormRow>
                <FormField
                  label={t('settings.tools.mcp.fields.allowed_domains')}
                  hint={t('settings.tools.mcp.fields.one_per_line')}
                >
                  <FormTextarea
                    className="min-h-24 font-mono text-xs"
                    value={allowedDomains}
                    onChange={(event) => setAllowedDomains(event.target.value)}
                  />
                </FormField>
                <FormField
                  label={t('settings.tools.mcp.fields.allowed_cidrs')}
                  hint={t('settings.tools.mcp.fields.one_per_line')}
                >
                  <FormTextarea
                    className="min-h-24 font-mono text-xs"
                    value={allowedCidrs}
                    onChange={(event) => setAllowedCidrs(event.target.value)}
                  />
                </FormField>
              </FormRow>
              <ToggleField
                label={t('settings.tools.mcp.fields.tls_verify')}
                checked={tlsVerify}
                onChange={setTlsVerify}
              />
            </>
          ) : (
            <p role="note" className="text-xs leading-5 text-tx-3">
              {t('settings.tools.mcp.editor.member_network')}
            </p>
          )}
          <p className="text-xs leading-5 text-tx-3">
            {t('settings.tools.mcp.fields.redirects_blocked')}
          </p>
        </FormSection>
      )}

      <FormSection title={t('settings.tools.mcp.editor.runtime')}>
        <FormRow>
          <FormField label={t('settings.tools.mcp.fields.timeout_ms')}>
            <FormInput
              type="number"
              min={1000}
              max={120000}
              value={timeoutMs}
              onChange={(event) => setTimeoutMs(event.target.value)}
            />
          </FormField>
          <FormField label={t('settings.tools.mcp.fields.max_response_bytes')}>
            <FormInput
              type="number"
              min={1024}
              max={16777216}
              value={maxResponseBytes}
              onChange={(event) => setMaxResponseBytes(event.target.value)}
            />
          </FormField>
        </FormRow>
        <ToggleField
          label={t('settings.tools.mcp.fields.enabled')}
          checked={enabled}
          onChange={setEnabled}
        />
      </FormSection>
    </FormDrawer>
  );
}

function teamOf(server: agentApi.McpServer): string | undefined {
  return server.scope.scope === 'team' ? server.scope.team_id : undefined;
}

function ownerOf(server: agentApi.McpServer): string | undefined {
  return server.scope.scope === 'personal' ? server.scope.owner_id : undefined;
}
