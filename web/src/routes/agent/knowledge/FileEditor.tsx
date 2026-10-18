import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Pencil, Sparkles, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import {
  deleteKnowledgePath,
  type KnowledgeScope,
  moveKnowledgeFile,
  readKnowledgeFile,
  scopeKey,
  writeKnowledgeFile,
} from '@/api/agent/knowledge';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { toast } from '@/shell/ui/sonner';

import { MarkdownMessage } from '../markdown';
import { isMarkdown } from './fileTree';
import { liveEditingLoses, MarkdownEditor } from '../markdownEditor/MarkdownEditor';

type FileMode = 'preview' | 'edit' | 'source';

/**
 * One knowledge file: read as Markdown (or as text), changed in place by those
 * who may change this base, renamed or deleted. Markdown is edited as it reads
 * ("- " turns into a bullet), or as source; a file with Markdown the live editor
 * would not keep is edited as source. A file being changed keeps its draft until
 * it is saved or given up.
 */
export function FileEditor({
  scope,
  path,
  canEdit,
  onMoved,
  onDeleted,
  onAskAi,
}: {
  scope: KnowledgeScope;
  path: string;
  canEdit: boolean;
  onMoved: (to: string) => void;
  onDeleted: () => void;
  /** Has Mole Agent change this file, in a chat of the base's scope. */
  onAskAi: () => void;
}) {
  const { t } = useTranslation('agent');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const fileKey = ['agent', 'knowledge', scopeKey(scope), 'file', path];
  const file = useQuery({
    queryKey: fileKey,
    queryFn: () => readKnowledgeFile(scope, path),
    retry: false,
  });
  const [mode, setMode] = React.useState<FileMode>('preview');
  const [draft, setDraft] = React.useState<string | null>(null);
  const markdown = isMarkdown(path);
  const stored = file.data?.content ?? '';
  // Asked only once the file is to be edited: the check writes the file out once.
  const sourceOnly = React.useMemo(
    () => markdown && mode === 'edit' && liveEditingLoses(stored),
    [markdown, mode, stored],
  );
  const [renaming, setRenaming] = React.useState<string | null>(null);
  const [deleting, setDeleting] = React.useState(false);

  const refresh = () => queryClient.invalidateQueries({ queryKey: ['agent', 'knowledge'] });
  const fail = (error: unknown) => toast.error(toApiError(error).message);

  const save = useMutation({
    mutationFn: (content: string) => writeKnowledgeFile(scope, path, content),
    onSuccess: async () => {
      toast.success(t('knowledge.saved'));
      setDraft(null);
      setMode('preview');
      await refresh();
    },
    onError: fail,
  });
  const move = useMutation({
    mutationFn: (to: string) => moveKnowledgeFile(scope, path, to),
    onSuccess: async (_, to) => {
      toast.success(t('knowledge.renamed'));
      setRenaming(null);
      await refresh();
      onMoved(to);
    },
    onError: fail,
  });
  const remove = useMutation({
    mutationFn: () => deleteKnowledgePath(scope, path),
    onSuccess: async () => {
      toast.success(t('knowledge.deleted'));
      setDeleting(false);
      await refresh();
      onDeleted();
    },
    onError: fail,
  });

  if (file.isLoading) return <ProductState variant="loading" />;
  if (file.isError) return <ProductState variant="error" error={file.error} />;
  const content = file.data?.content ?? '';
  const text = draft ?? content;
  const dirty = draft !== null && draft !== content;

  return (
    <div data-agent-knowledge-editor={path} className="flex h-full min-h-0 flex-col">
      <div className="flex min-h-[44px] flex-wrap items-center gap-2 border-b border-bd-0 px-3 py-1.5">
        {renaming === null ? (
          <span className="min-w-0 truncate font-mono text-xs font-strong text-tx-0">{path}</span>
        ) : (
          <form
            className="flex min-w-0 flex-1 items-center gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              if (renaming.trim() && renaming.trim() !== path) move.mutate(renaming.trim());
            }}
          >
            <input
              aria-label={t('knowledge.rename_to')}
              value={renaming}
              autoFocus
              onChange={(event) => setRenaming(event.target.value)}
              className="h-8 min-w-0 flex-1 rounded-md bg-bg-2 px-2.5 font-mono text-xs text-tx-0"
            />
            <ChromeButton type="submit" size="sm" variant="primary" disabled={move.isPending}>
              {t('knowledge.rename')}
            </ChromeButton>
            <ChromeButton type="button" size="sm" onClick={() => setRenaming(null)}>
              {tc('actions.cancel')}
            </ChromeButton>
          </form>
        )}
        {file.data && renaming === null && (
          <span className="type-micro text-tx-3">
            {t('knowledge.updated', { time: formatMicrosActive(file.data.updated_at_micros) })}
          </span>
        )}
        <span className="flex-1" />
        {canEdit && renaming === null && (
          <>
            <ViewTabs<FileMode>
              label={t('knowledge.mode_label')}
              value={mode}
              onValueChange={setMode}
              items={[
                { key: 'preview', label: t('knowledge.preview') },
                { key: 'edit', label: t('knowledge.edit') },
                ...(markdown ? [{ key: 'source' as const, label: t('knowledge.source') }] : []),
              ]}
            />
            <ChromeButton size="sm" title={t('knowledge.ai_hint')} onClick={onAskAi}>
              <Sparkles className="h-3.5 w-3.5" />
              {t('knowledge.ask_ai')}
            </ChromeButton>
            <ChromeButton
              size="sm"
              aria-label={t('knowledge.rename')}
              title={t('knowledge.rename')}
              onClick={() => setRenaming(path)}
            >
              <Pencil className="h-3.5 w-3.5" />
            </ChromeButton>
            <ChromeButton
              size="sm"
              aria-label={t('knowledge.delete')}
              title={t('knowledge.delete')}
              onClick={() => setDeleting(true)}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </ChromeButton>
          </>
        )}
      </div>

      <div className="flex min-h-0 flex-1 flex-col overflow-auto">
        {canEdit && mode === 'edit' && markdown && !sourceOnly ? (
          <MarkdownEditor
            value={text}
            onChange={setDraft}
            ariaLabel={t('knowledge.edit_label', { path })}
            className="min-h-full flex-1 px-4 py-3"
          />
        ) : canEdit && mode !== 'preview' ? (
          <>
            {sourceOnly && (
              <p data-agent-knowledge-source-only className="px-4 pt-3 text-xs text-tx-2">
                {t('knowledge.source_only')}
              </p>
            )}
            <textarea
              aria-label={
                markdown ? t('knowledge.source_label', { path }) : t('knowledge.edit_label', { path })
              }
              value={text}
              onChange={(event) => setDraft(event.target.value)}
              spellCheck={false}
              className="block min-h-[320px] w-full flex-1 resize-none bg-transparent px-4 py-3 font-mono text-xs leading-5 text-tx-0"
            />
          </>
        ) : markdown ? (
          <div className="px-4 py-3 font-sans text-sm leading-6 text-tx-1">
            <MarkdownMessage content={text} />
          </div>
        ) : (
          <pre className="whitespace-pre-wrap break-words px-4 py-3 font-mono text-xs leading-5 text-tx-1">
            {text}
          </pre>
        )}
      </div>

      {canEdit && dirty && (
        <div className="flex items-center justify-end gap-2 border-t border-bd-0 px-3 py-2">
          <span className="mr-auto text-xs text-tx-2">{t('knowledge.unsaved')}</span>
          <ChromeButton size="sm" onClick={() => setDraft(null)} disabled={save.isPending}>
            {t('knowledge.discard')}
          </ChromeButton>
          <ChromeButton size="sm" variant="primary" onClick={() => save.mutate(text)} disabled={save.isPending}>
            {t('knowledge.save')}
          </ChromeButton>
        </div>
      )}

      <ConfirmDialog
        open={deleting}
        onOpenChange={setDeleting}
        destructive
        busy={remove.isPending}
        title={t('knowledge.delete_title', { path })}
        description={t('knowledge.delete_confirm')}
        cancelLabel={tc('actions.cancel')}
        confirmLabel={t('knowledge.delete')}
        busyLabel={t('knowledge.deleting')}
        onConfirm={() => remove.mutate()}
      />
    </div>
  );
}
