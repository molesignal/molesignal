import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link as LinkIcon, Pencil, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import {
  deleteMemory,
  listMemories,
  MEMORY_KINDS,
  type Memory,
  memorySettings,
  setMemorySettings,
} from '@/api/agent/memory';
import * as teamsApi from '@/api/teams';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import { useAuthStore } from '@/stores/auth';

import { MemoryEditor } from './MemoryEditor';

const PERSONAL = 'personal';

/**
 * What Mole Agent keeps from chats, by whose memory it is: the person's own, or
 * a team's they are in. Grouped by kind; each can be changed or deleted. Nothing
 * is created here: memories are written by chats.
 */
export function MemoryView() {
  const { t } = useTranslation('agent');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const userId = useAuthStore((state) => state.ctx?.user_id ?? '');
  const [owner, setOwner] = React.useState(PERSONAL);
  const [editing, setEditing] = React.useState<Memory | null>(null);
  const [deleting, setDeleting] = React.useState<Memory | null>(null);
  const teamId = owner === PERSONAL ? null : owner;

  const teams = useQuery({ queryKey: ['teams'], queryFn: teamsApi.list, retry: false, staleTime: 300_000 });
  const memories = useQuery({
    queryKey: ['agent', 'memories', owner],
    queryFn: () => listMemories({ teamId }),
    retry: false,
  });
  const settings = useQuery({ queryKey: ['agent', 'memory-settings'], queryFn: memorySettings, retry: false });

  const fail = (error: unknown) => toast.error(toApiError(error).message);
  const remove = useMutation({
    mutationFn: (memory: Memory) => deleteMemory(memory.id),
    onSuccess: async () => {
      toast.success(t('memory.deleted'));
      setDeleting(null);
      await queryClient.invalidateQueries({ queryKey: ['agent', 'memories'] });
    },
    onError: fail,
  });
  const distill = useMutation({
    mutationFn: setMemorySettings,
    onSuccess: (next) => queryClient.setQueryData(['agent', 'memory-settings'], next),
    onError: fail,
  });

  const myTeams = (teams.data ?? []).filter((team) => team.member_ids.includes(userId));
  const options = [
    { value: PERSONAL, label: t('memory.personal') },
    ...myTeams.map((team) => ({ value: team.id, label: team.name })),
  ];
  const rows = memories.data ?? [];

  return (
    <div data-agent-memory className="flex h-full min-h-0 flex-col">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-bd-0 px-4 py-2.5">
        <FittedSelect
          aria-label={t('memory.scope_label')}
          value={owner}
          onValueChange={setOwner}
          options={options}
          maxWidth={260}
        />
        <p className="min-w-0 flex-1 text-xs leading-5 text-tx-2">{t('memory.description')}</p>
        {settings.data && (
          <label className="flex items-center gap-2 text-xs text-tx-1" title={t('memory.distill_hint')}>
            <Switch
              checked={settings.data.distill_after_chat}
              disabled={!settings.data.can_edit || distill.isPending}
              onCheckedChange={(checked) => distill.mutate(checked)}
              aria-label={t('memory.distill')}
            />
            <span className="font-strong">{t('memory.distill')}</span>
          </label>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-auto px-4 py-3">
        {memories.isLoading ? (
          <ProductState variant="loading" />
        ) : memories.isError ? (
          <ProductState variant="error" error={memories.error} />
        ) : rows.length === 0 ? (
          <p data-agent-memory-empty className="py-8 text-center text-xs text-tx-3">
            {t('memory.empty')}
          </p>
        ) : (
          <div className="flex flex-col gap-5">
            {MEMORY_KINDS.map((kind) => {
              const group = rows.filter((memory) => memory.kind === kind);
              if (group.length === 0) return null;
              return (
                <section key={kind} data-agent-memory-kind={kind} aria-labelledby={`memory-${kind}`}>
                  <div className="mb-1.5 flex items-baseline gap-2">
                    <h3 id={`memory-${kind}`} className="text-xs font-strong text-tx-1">
                      {t(`memory.kinds.${kind}`)}
                    </h3>
                    <span className="type-micro text-tx-3">{t(`memory.kind_hints.${kind}`)}</span>
                  </div>
                  <ul className="divide-y divide-bd-0 overflow-hidden rounded-lg bg-bg-2">
                    {group.map((memory) => (
                      <li key={memory.id} data-agent-memory-row={memory.name} className="flex items-start gap-3 px-3 py-2.5">
                        <details className="min-w-0 flex-1">
                          {/* Every line on one rhythm: the same line height, the same gap. */}
                          <summary className="cursor-pointer list-none [&::-webkit-details-marker]:hidden">
                            <span className="block text-sm font-strong leading-5 text-tx-0">{memory.name}</span>
                            <span className="mt-1 block text-xs leading-5 text-tx-2">{memory.description}</span>
                          </summary>
                          <p className="mt-1 whitespace-pre-wrap text-xs leading-5 text-tx-1">{memory.body}</p>
                          <p className="mt-1 flex flex-wrap items-center gap-2 type-micro leading-5 text-tx-3">
                            {t('memory.updated', { time: formatMicrosActive(memory.updated_at_micros) })}
                            {memory.source_chat_id && memory.source_chat_title != null && (
                              <Link
                                data-agent-memory-source
                                to={`/agent/chat?chat=${encodeURIComponent(memory.source_chat_id)}`}
                                title={t('memory.from_chat')}
                                className="inline-flex min-w-0 max-w-full items-center gap-1 text-indigo hover:underline"
                              >
                                <LinkIcon aria-hidden="true" className="h-3 w-3 shrink-0" />
                                <span className="truncate">{memory.source_chat_title || t('untitled')}</span>
                              </Link>
                            )}
                          </p>
                        </details>
                        <ChromeButton
                          size="sm"
                          aria-label={t('memory.edit')}
                          title={t('memory.edit')}
                          onClick={() => setEditing(memory)}
                        >
                          <Pencil className="h-3.5 w-3.5" />
                        </ChromeButton>
                        <ChromeButton
                          size="sm"
                          aria-label={t('memory.delete')}
                          title={t('memory.delete')}
                          onClick={() => setDeleting(memory)}
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </ChromeButton>
                      </li>
                    ))}
                  </ul>
                </section>
              );
            })}
          </div>
        )}
      </div>

      {editing && <MemoryEditor memory={editing} onClose={() => setEditing(null)} />}
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        destructive
        busy={remove.isPending}
        title={t('memory.delete_title', { name: deleting?.name ?? '' })}
        description={t('memory.delete_confirm')}
        cancelLabel={tc('actions.cancel')}
        confirmLabel={t('memory.delete')}
        busyLabel={t('memory.deleting')}
        onConfirm={() => deleting && remove.mutate(deleting)}
      />
    </div>
  );
}
