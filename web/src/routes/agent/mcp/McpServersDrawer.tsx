import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ChevronDown,
  ChevronRight,
  Ellipsis,
  FileInput,
  FlaskConical,
  KeyRound,
  Pencil,
  Plus,
  Power,
  RefreshCw,
  Server,
  Unplug,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as agentApi from '@/api/agent';
import { toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { FormDrawer } from '@/shell/FormDrawer';
import { Button } from '@/shell/ui/button';
import { Checkbox } from '@/shell/ui/checkbox';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';

import { McpImportDrawer } from './McpImportDrawer';
import { McpRemovalDrawer, type McpRemovalTarget } from './McpRemovalDrawer';
import { McpServerDetails } from './McpServerDetails';
import { McpServerEditor } from './McpServerEditor';
import { McpStatusBadge } from './McpStatusBadge';
import { MyCredentialDrawer } from './MyCredentialDrawer';
import { useScopes } from '../scope/useScopes';

function toggle(values: string[], value: string): string[] {
  return values.includes(value) ? values.filter((item) => item !== value) : [...values, value];
}

/**
 * The MCP servers the signed-in person can see: the organization's, their
 * teams', their own (and, for an administrator, everyone's). Each says whose
 * it is; the actions follow what the person may do with it.
 */
export function McpServersDrawer({
  open,
  onClose,
  onChanged,
}: {
  open: boolean;
  onClose: () => void;
  onChanged: () => Promise<void>;
}) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const scopes = useScopes();
  const [editor, setEditor] = React.useState<agentApi.McpServer | 'new' | null>(null);
  const [importing, setImporting] = React.useState(false);
  const [credentialFor, setCredentialFor] = React.useState<agentApi.McpServer | null>(null);
  const [expanded, setExpanded] = React.useState<string | null>(null);
  const [discoveries, setDiscoveries] = React.useState<
    Record<string, agentApi.DiscoveredMcpTool[]>
  >({});
  const [selected, setSelected] = React.useState<Record<string, string[]>>({});
  const [removalTarget, setRemovalTarget] = React.useState<McpRemovalTarget | null>(null);
  const servers = useQuery({
    queryKey: ['agent', 'mcp-servers'],
    queryFn: agentApi.listMcpServers,
    enabled: open,
    retry: false,
  });
  const stdioCommands =
    useQuery({
      queryKey: ['agent', 'mcp-stdio-commands'],
      queryFn: agentApi.mcpStdioCommands,
      enabled: open,
      staleTime: 60_000,
      retry: false,
    }).data ?? [];

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ['agent', 'mcp-servers'] }),
      queryClient.invalidateQueries({ queryKey: ['agent', 'mcp-server'] }),
      queryClient.invalidateQueries({ queryKey: ['agent', 'commands'] }),
      onChanged(),
    ]);
  };
  const fail = (error: unknown) => toast.error(toApiError(error).message);

  const testMutation = useMutation({
    mutationFn: agentApi.testMcpServer,
    onSuccess: async (result) => {
      if (result.success) {
        setDiscoveries((current) => ({ ...current, [result.server.id]: result.discovered_tools }));
        setSelected((current) => ({
          ...current,
          [result.server.id]: result.discovered_tools.map((tool) => tool.name),
        }));
        toast.success(
          t('settings.tools.mcp.feedback.test_success', { count: result.discovered_tools.length }),
        );
      } else {
        toast.error(result.error ?? t('settings.tools.mcp.feedback.test_failed'));
      }
      await servers.refetch();
    },
    onError: fail,
  });
  const syncMutation = useMutation({
    mutationFn: ({ id, tools }: { id: string; tools: string[] }) =>
      agentApi.syncMcpServer(id, tools),
    onSuccess: async (result) => {
      setDiscoveries((current) => ({ ...current, [result.server.id]: [] }));
      toast.success(t('settings.tools.mcp.feedback.sync_success', { count: result.tools.length }));
      setExpanded(result.server.id);
      await refresh();
    },
    onError: fail,
  });
  const enabledMutation = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      agentApi.setMcpServerEnabled(id, enabled),
    onSuccess: async (server) => {
      toast.success(
        t(server.enabled ? 'settings.tools.mcp.feedback.enabled' : 'settings.tools.mcp.feedback.disabled'),
      );
      await refresh();
    },
    onError: fail,
  });
  const deleteMutation = useMutation({
    mutationFn: agentApi.deleteMcpServer,
    onSuccess: async () => {
      setRemovalTarget(null);
      toast.success(t('settings.tools.mcp.feedback.deleted'));
      await refresh();
    },
    onError: fail,
  });
  const inspectRemovalMutation = useMutation({
    mutationFn: async (server: agentApi.McpServer) => {
      const detail = await agentApi.getMcpServer(server.id);
      const dependencies = await Promise.all(
        detail.tools.map((tool) => agentApi.getToolDependencies(tool.id || tool.name)),
      );
      return { server, tools: detail.tools, dependencies };
    },
    onSuccess: setRemovalTarget,
    onError: fail,
  });

  return (
    <>
      <FormDrawer
        open={open}
        onOpenChange={(next) => {
          if (!next) onClose();
        }}
        title={t('settings.tools.mcp.title')}
        subtitle={t('settings.tools.mcp.description')}
        width={760}
        footer={
          <>
            <Button variant="outline" onClick={() => setImporting(true)}>
              <FileInput className="h-3.5 w-3.5" />
              {t('settings.tools.mcp.import.open')}
            </Button>
            <Button onClick={() => setEditor('new')}>
              <Plus className="h-3.5 w-3.5" />
              {t('settings.tools.actions.add_mcp')}
            </Button>
          </>
        }
      >
        {servers.isLoading ? (
          <ProductState variant="loading" compact />
        ) : servers.isError ? (
          <ProductState variant="error" error={servers.error} compact />
        ) : servers.data?.length ? (
          <div className="space-y-3">
            {servers.data.map((server) => {
              const discovered = discoveries[server.id] ?? [];
              const selectedNames = selected[server.id] ?? [];
              const open = expanded === server.id;
              const memberCredential = server.auth_type === 'member_secret';
              return (
                <section
                  key={server.id}
                  data-agent-mcp-server={server.handle}
                  className="overflow-hidden rounded-lg bg-bg-1"
                >
                  <div className="flex flex-wrap items-center gap-3 px-4 py-3">
                    <span className="grid h-9 w-9 place-items-center rounded-md bg-bg-2">
                      <Server className="h-4 w-4 text-tx-2" />
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-strong text-tx-0">{server.name}</span>
                        <McpStatusBadge status={server.status} />
                        <span
                          data-agent-mcp-scope={server.scope.scope}
                          className="rounded bg-bg-3 px-1.5 text-type-micro text-tx-2"
                        >
                          {scopes.label({
                            scope: server.scope.scope,
                            team_id: server.scope.scope === 'team' ? server.scope.team_id : null,
                            owner_id:
                              server.scope.scope === 'personal' ? server.scope.owner_id : null,
                          })}
                        </span>
                        {!server.enabled && (
                          <span className="rounded bg-bg-3 px-1.5 text-type-micro text-tx-3">
                            {t('status.disabled')}
                          </span>
                        )}
                      </div>
                      <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-xs text-tx-3">
                        <span>{t(`settings.tools.mcp.transport.${server.transport}`)}</span>
                        <span>
                          {t('settings.tools.mcp.tool_count', { count: server.tool_count ?? 0 })}
                        </span>
                        {(server.prompt_count ?? 0) > 0 && (
                          <span>
                            {t('settings.tools.mcp.prompt_count', {
                              count: server.prompt_count ?? 0,
                            })}
                          </span>
                        )}
                        {server.transport === 'stdio' &&
                          (server.scope.scope !== 'org' ||
                            !stdioCommands.includes(
                              agentApi.stdioProgram(server.command_template ?? ''),
                            )) && (
                            <span
                              data-agent-mcp-stdio-blocked
                              className="text-[var(--yellow-ink)]"
                            >
                              {t('settings.tools.mcp.stdio_not_allowed')}
                            </span>
                          )}
                        {server.last_error && (
                          <span className="text-[var(--red-ink)]">{server.last_error}</span>
                        )}
                      </div>
                    </div>
                    {memberCredential && (
                      <Button
                        size="sm"
                        variant="outline"
                        data-agent-mcp-my-credential={server.my_credential ? 'saved' : 'missing'}
                        onClick={() => setCredentialFor(server)}
                      >
                        <KeyRound className="h-3.5 w-3.5" />
                        {server.my_credential
                          ? t('settings.tools.mcp.my_credential.button_saved', {
                              last4: server.my_credential.last4,
                            })
                          : t('settings.tools.mcp.my_credential.button_missing')}
                      </Button>
                    )}
                    {server.editable && (
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => testMutation.mutate(server.id)}
                        disabled={testMutation.isPending}
                      >
                        <FlaskConical className="h-3.5 w-3.5" />
                        {t('settings.tools.mcp.test_connection')}
                      </Button>
                    )}
                    {(server.editable || server.moderatable) && (
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon" aria-label={t('common.actions')}>
                            <Ellipsis className="h-4 w-4" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          {server.editable && (
                            <DropdownMenuItem onSelect={() => setEditor(server)}>
                              <Pencil className="h-3.5 w-3.5" />
                              {t('common.edit')}
                            </DropdownMenuItem>
                          )}
                          {server.moderatable && (
                            <DropdownMenuItem
                              onSelect={() =>
                                enabledMutation.mutate({ id: server.id, enabled: !server.enabled })
                              }
                            >
                              <Power className="h-3.5 w-3.5" />
                              {t(
                                server.enabled
                                  ? 'settings.tools.mcp.disable'
                                  : 'settings.tools.mcp.enable',
                              )}
                            </DropdownMenuItem>
                          )}
                          {server.moderatable && (
                            <DropdownMenuItem
                              className="text-[var(--red-ink)]"
                              disabled={inspectRemovalMutation.isPending}
                              onSelect={() => inspectRemovalMutation.mutate(server)}
                            >
                              <Unplug className="h-3.5 w-3.5" />
                              {t('settings.tools.mcp.remove')}
                            </DropdownMenuItem>
                          )}
                        </DropdownMenuContent>
                      </DropdownMenu>
                    )}
                  </div>
                  {discovered.length > 0 && (
                    <div className="bg-bg-2 p-4">
                      <div className="flex items-center gap-2">
                        <span className="text-xs font-strong text-tx-1">
                          {t('settings.tools.mcp.discovered', { count: discovered.length })}
                        </span>
                        <span className="text-xs text-tx-3">
                          {t('settings.tools.mcp.new_tools_disabled')}
                        </span>
                        <Button
                          size="sm"
                          className="ml-auto"
                          onClick={() =>
                            syncMutation.mutate({ id: server.id, tools: selectedNames })
                          }
                          disabled={selectedNames.length === 0 || syncMutation.isPending}
                        >
                          <RefreshCw className="h-3.5 w-3.5" />
                          {t('settings.tools.mcp.sync_selected')}
                        </Button>
                      </div>
                      <div className="mt-3 max-h-60 divide-y divide-bd-0 overflow-auto rounded-md bg-bg-1">
                        {discovered.map((tool) => (
                          <label
                            key={tool.name}
                            className="flex cursor-pointer items-start gap-3 px-3 py-2.5"
                          >
                            <Checkbox
                              checked={selectedNames.includes(tool.name)}
                              onCheckedChange={() =>
                                setSelected((current) => ({
                                  ...current,
                                  [server.id]: toggle(current[server.id] ?? [], tool.name),
                                }))
                              }
                            />
                            <span className="min-w-0">
                              <span className="block font-mono text-xs font-strong text-tx-0">
                                {tool.name}
                              </span>
                              <span className="mt-0.5 block text-xs text-tx-3">
                                {tool.description}
                              </span>
                            </span>
                          </label>
                        ))}
                      </div>
                    </div>
                  )}
                  {((server.tool_count ?? 0) > 0 || (server.prompt_count ?? 0) > 0) && (
                    <>
                      <button
                        type="button"
                        aria-expanded={open}
                        onClick={() => setExpanded(open ? null : server.id)}
                        className="flex w-full items-center gap-1.5 px-4 py-2 text-left text-xs font-strong text-tx-2 hover:bg-bg-2 hover:text-tx-0"
                      >
                        {open ? (
                          <ChevronDown className="h-3.5 w-3.5" />
                        ) : (
                          <ChevronRight className="h-3.5 w-3.5" />
                        )}
                        {t('settings.tools.mcp.details.toggle')}
                      </button>
                      {open && <McpServerDetails server={server} onChanged={refresh} />}
                    </>
                  )}
                </section>
              );
            })}
          </div>
        ) : (
          <div className="px-6 py-12 text-center">
            <Server className="mx-auto h-6 w-6 text-tx-3" />
            <div className="mt-3 text-sm font-strong text-tx-1">
              {t('settings.tools.mcp.empty_title')}
            </div>
            <p className="mt-1 text-xs text-tx-3">{t('settings.tools.mcp.empty_description')}</p>
          </div>
        )}
      </FormDrawer>
      <McpServerEditor target={editor} onClose={() => setEditor(null)} onSaved={refresh} />
      <McpImportDrawer
        open={importing}
        onClose={() => setImporting(false)}
        onImported={refresh}
      />
      <MyCredentialDrawer
        server={credentialFor}
        onClose={() => setCredentialFor(null)}
        onSaved={refresh}
      />
      <McpRemovalDrawer
        target={removalTarget}
        pending={deleteMutation.isPending}
        onClose={() => setRemovalTarget(null)}
        onConfirm={(server) => deleteMutation.mutate(server.id)}
      />
    </>
  );
}
