import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ChevronDown, CircleAlert, FilePlus2, FolderPlus, FolderUp, Upload, Wand2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import {
  ENTRY_FILE,
  type KnowledgeBase,
  type KnowledgeLimits,
  type KnowledgeScope,
  listKnowledgeFiles,
  scopeKey,
  uploadKnowledge,
  writeKnowledgeFile,
} from '@/api/agent/knowledge';
import { toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';

import { FileEditor } from './FileEditor';
import { buildTree, parentFolders, uploadPath } from './fileTree';
import { FileTreeNav } from './FileTreeNav';
import { formatSize } from './KnowledgeView';

/** The folder-picking attribute React does not know by name. */
const FOLDER_PICKER = { webkitdirectory: '', directory: '' } as Record<string, string>;

/**
 * One knowledge base: its files as a tree beside the file open, the quota it
 * is within, and the ways to fill it: write a file, upload files or a folder,
 * have Mole Agent change it, or interview the person with /init.
 */
export function KnowledgeExplorer({
  base,
  scope,
  limits,
  initialPath = null,
}: {
  base: KnowledgeBase;
  scope: KnowledgeScope;
  limits: KnowledgeLimits;
  /** The file a link opened the page on; the entry file when it is not there. */
  initialPath?: string | null;
}) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const files = useQuery({
    queryKey: ['agent', 'knowledge', scopeKey(scope), 'files'],
    queryFn: () => listKnowledgeFiles(scope),
    retry: false,
  });
  const [selected, setSelected] = React.useState<string | null>(initialPath);
  const [open, setOpen] = React.useState<ReadonlySet<string>>(
    () => new Set(initialPath ? parentFolders(initialPath) : []),
  );
  // What is being made, by its path: a file, or a folder named before its first file.
  const [creating, setCreating] = React.useState<{ kind: 'file' | 'folder'; path: string } | null>(
    null,
  );
  const filesInput = React.useRef<HTMLInputElement>(null);
  const folderInput = React.useRef<HTMLInputElement>(null);

  const rows = React.useMemo(() => files.data?.files ?? [], [files.data]);
  const canEdit = files.data?.can_edit ?? base.can_edit;
  const tree = React.useMemo(() => buildTree(rows), [rows]);
  const name = base.scope === 'org' ? t('knowledge.org') : (base.team_name ?? '');

  // The entry file opens first; a file that is gone closes. Nothing is chosen
  // before the files are read, so a file a link named is kept until then.
  const loaded = files.data !== undefined;
  React.useEffect(() => {
    if (!loaded) return;
    if (selected && rows.some((file) => file.path === selected)) return;
    const entry = rows.find((file) => file.path === ENTRY_FILE) ?? rows[0];
    setSelected(entry?.path ?? null);
  }, [loaded, rows, selected]);

  const show = (path: string) => {
    setOpen((current) => new Set([...current, ...parentFolders(path)]));
    setSelected(path);
  };

  const refresh = () => queryClient.invalidateQueries({ queryKey: ['agent', 'knowledge'] });
  const fail = (error: unknown) => toast.error(toApiError(error).message);

  const create = useMutation({
    mutationFn: (path: string) => writeKnowledgeFile(scope, path, path === ENTRY_FILE ? `# ${name}\n` : ''),
    onSuccess: async (_, path) => {
      setCreating(null);
      await refresh();
      show(path);
    },
    onError: fail,
  });
  const upload = useMutation({
    mutationFn: (picked: File[]) =>
      uploadKnowledge(
        scope,
        picked.map((file) => ({ path: uploadPath(file), file })),
      ),
    onSuccess: async (outcome) => {
      if (outcome.written > 0) toast.success(t('knowledge.uploaded', { count: outcome.written }));
      if (outcome.skipped.length > 0) {
        toast.warning(
          t('knowledge.skipped', {
            count: outcome.skipped.length,
            paths: outcome.skipped
              .slice(0, 3)
              .map((item) => `${item.path} (${t(`knowledge.skip_reasons.${item.reason}`)})`)
              .join(', '),
          }),
        );
      }
      await refresh();
    },
    onError: fail,
  });

  const pick = (event: React.ChangeEvent<HTMLInputElement>) => {
    const picked = [...(event.target.files ?? [])];
    event.target.value = '';
    if (picked.length > 0) upload.mutate(picked);
  };

  // A chat of the base's scope: a team's knowledge is changed in a chat for the team.
  const ask = (prompt: string) =>
    useMoleAgentStore.getState().start({
      prompt,
      teamId: base.scope === 'team' ? (base.team_id ?? null) : null,
    });

  return (
    <div className="flex h-full min-h-0 flex-col" data-agent-knowledge-explorer={scopeKey(scope)}>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-bd-0 px-4 py-2.5">
        <div className="min-w-0">
          <h2 className="truncate text-sm font-strong text-tx-0">{name}</h2>
          <p className="type-micro text-tx-3">
            {t('knowledge.quota', {
              files: base.files,
              maxFiles: limits.max_files,
              size: formatSize(base.bytes),
              maxSize: formatSize(limits.max_total_bytes),
            })}
          </p>
        </div>
        <span className="flex-1" />
        {canEdit && (
          <div className="flex flex-wrap items-center gap-1.5">
            {/* A file or a folder by its path: "runbooks/api-5xx.md" makes both. */}
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <ChromeButton size="sm">
                  <FilePlus2 className="h-3.5 w-3.5" />
                  {t('knowledge.new')}
                  <ChevronDown className="h-3.5 w-3.5 text-tx-3" />
                </ChromeButton>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onSelect={() => setCreating({ kind: 'file', path: '' })}>
                  <FilePlus2 className="h-3.5 w-3.5" />
                  {t('knowledge.new_file')}
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setCreating({ kind: 'folder', path: '' })}>
                  <FolderPlus className="h-3.5 w-3.5" />
                  {t('knowledge.new_folder')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <ChromeButton size="sm" disabled={upload.isPending}>
                  <Upload className="h-3.5 w-3.5" />
                  {t('knowledge.upload')}
                  <ChevronDown className="h-3.5 w-3.5 text-tx-3" />
                </ChromeButton>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onSelect={() => filesInput.current?.click()}>
                  <Upload className="h-3.5 w-3.5" />
                  {t('knowledge.upload_files')}
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => folderInput.current?.click()}>
                  <FolderUp className="h-3.5 w-3.5" />
                  {t('knowledge.upload_folder')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        )}
        <input ref={filesInput} type="file" multiple hidden onChange={pick} aria-label={t('knowledge.upload_files')} />
        <input
          ref={folderInput}
          type="file"
          multiple
          hidden
          onChange={pick}
          aria-label={t('knowledge.upload_folder')}
          {...FOLDER_PICKER}
        />
      </div>

      {base.changed_since_entry > 0 && (
        <div className="flex flex-wrap items-center gap-2 bg-[var(--yellow-tint)] px-4 py-2 text-xs text-[var(--yellow-ink)]">
          <CircleAlert aria-hidden="true" className="h-3.5 w-3.5 shrink-0" />
          <span className="min-w-0 flex-1">{t('knowledge.stale', { count: base.changed_since_entry })}</span>
          {canEdit && (
            <ChromeButton size="sm" onClick={() => ask(t('knowledge.update_entry_prompt'))}>
              {t('knowledge.update_entry')}
            </ChromeButton>
          )}
        </div>
      )}
      {!canEdit && (
        <p className="border-b border-bd-0 px-4 py-2 text-xs text-tx-2">{t('knowledge.read_only')}</p>
      )}
      {creating !== null && (
        <form
          data-agent-knowledge-create={creating.kind}
          className="flex flex-wrap items-center gap-2 border-b border-bd-0 px-4 py-2"
          onSubmit={(event) => {
            event.preventDefault();
            const path = creating.path.trim().replace(/^\/+/, '');
            if (!path) return;
            // A folder is there once a file is in it: its name goes before the first file's.
            if (creating.kind === 'folder' || path.endsWith('/')) {
              setCreating({ kind: 'file', path: `${path.replace(/\/+$/, '')}/` });
              return;
            }
            create.mutate(path);
          }}
        >
          <input
            aria-label={t(creating.kind === 'folder' ? 'knowledge.new_folder_path' : 'knowledge.new_file_path')}
            placeholder={t(creating.kind === 'folder' ? 'knowledge.new_folder_hint' : 'knowledge.new_file_hint')}
            value={creating.path}
            autoFocus
            onChange={(event) => setCreating({ ...creating, path: event.target.value })}
            className="h-8 min-w-0 flex-1 rounded-md bg-bg-2 px-2.5 font-mono text-xs text-tx-0 placeholder:text-tx-3"
          />
          <ChromeButton
            type="submit"
            size="sm"
            variant="primary"
            disabled={create.isPending || !creating.path.trim()}
          >
            {t(creating.kind === 'folder' ? 'knowledge.next' : 'knowledge.create')}
          </ChromeButton>
          <ChromeButton type="button" size="sm" onClick={() => setCreating(null)}>
            {t('knowledge.cancel')}
          </ChromeButton>
          {creating.kind === 'file' && creating.path.endsWith('/') && (
            <p className="type-micro w-full text-tx-3">{t('knowledge.folder_first_file')}</p>
          )}
        </form>
      )}

      {files.isLoading ? (
        <ProductState variant="loading" />
      ) : files.isError ? (
        <ProductState variant="error" error={files.error} />
      ) : rows.length === 0 ? (
        <div data-agent-knowledge-empty className="mx-auto flex max-w-lg flex-col items-center gap-3 px-6 py-12 text-center">
          <p className="text-sm font-strong text-tx-0">{t('knowledge.empty_title')}</p>
          <p className="text-xs leading-5 text-tx-2">{t('knowledge.empty_description')}</p>
          {canEdit && (
            <div className="flex flex-wrap justify-center gap-2">
              <ChromeButton size="sm" variant="primary" onClick={() => ask('/init')}>
                <Wand2 className="h-3.5 w-3.5" />
                {t('knowledge.init')}
              </ChromeButton>
              <ChromeButton size="sm" onClick={() => create.mutate(ENTRY_FILE)} disabled={create.isPending}>
                <FilePlus2 className="h-3.5 w-3.5" />
                {t('knowledge.new_entry')}
              </ChromeButton>
            </div>
          )}
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col md:flex-row">
          <div className="shrink-0 overflow-auto border-b border-bd-0 p-2 md:w-[240px] md:border-b-0 md:border-r">
            <FileTreeNav
              nodes={tree}
              selected={selected}
              open={open}
              onToggle={(path) =>
                setOpen((current) => {
                  const next = new Set(current);
                  if (next.has(path)) next.delete(path);
                  else next.add(path);
                  return next;
                })
              }
              onSelect={show}
            />
          </div>
          <div className="min-h-0 min-w-0 flex-1">
            {selected ? (
              <FileEditor
                key={selected}
                scope={scope}
                path={selected}
                canEdit={canEdit}
                onMoved={show}
                onDeleted={() => setSelected(null)}
                onAskAi={() => ask(t('knowledge.ai_prompt', { path: selected }))}
              />
            ) : (
              <p className="px-4 py-6 text-xs text-tx-3">{t('knowledge.select_file')}</p>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
