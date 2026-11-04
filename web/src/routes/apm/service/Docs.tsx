import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { BookOpen, FilePlus2, FileText } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { listKnowledgeFiles, type KnowledgeScope, scopeKey, writeKnowledgeFile } from '@/api/agent/knowledge';
import { toApiError } from '@/lib/http';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';

import { knowledgeHref, serviceDocsFolder } from './links';

const ORG: KnowledgeScope = { scope: 'org' };

function useBaseFiles(scope: KnowledgeScope, enabled: boolean) {
  return useQuery({
    queryKey: ['agent', 'knowledge', scopeKey(scope), 'files'],
    queryFn: () => listKnowledgeFiles(scope),
    enabled,
    retry: false,
    staleTime: 60_000,
  });
}

/**
 * The service's documents: the files of the knowledge base kept under
 * `services/<name>/`, the organization's and its owning team's. Mole Agent
 * reads the same files when it looks into the service. A runbook can be
 * started from here.
 */
export function ServiceDocs({ name, teamId }: { name: string; teamId: string | null | undefined }) {
  const { t } = useTranslation('apm');
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const folder = serviceDocsFolder(name);
  const team: KnowledgeScope | null = teamId ? { scope: 'team', teamId: teamId } : null;
  const org = useBaseFiles(ORG, Boolean(name));
  const teamFiles = useBaseFiles(team ?? ORG, Boolean(name && team));

  const docs = [
    ...(org.data?.files ?? []).filter((file) => file.path.startsWith(folder)).map((file) => ({ base: 'org', path: file.path })),
    ...(team ? (teamFiles.data?.files ?? []) : []).filter((file) => file.path.startsWith(folder)).map((file) => ({ base: scopeKey(team!), path: file.path })),
  ];

  const runbook = `${folder}runbook.md`;
  const start = useMutation({
    mutationFn: () => writeKnowledgeFile(ORG, runbook, t('docs.runbook_template', { name })),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['agent', 'knowledge'] });
      navigate(knowledgeHref('org', runbook));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const hasRunbook = docs.some((doc) => doc.base === 'org' && doc.path === runbook);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          data-apm-docs=""
          className="inline-flex h-7 items-center gap-1.5 rounded-md bg-[var(--control-surface)] px-2 font-sans text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3 data-[state=open]:bg-bg-3"
        >
          <BookOpen aria-hidden className="h-3.5 w-3.5 text-tx-3" />
          {t('docs.title')}
          <span className="tabular-nums text-tx-3">{docs.length}</span>
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-72">
        <DropdownMenuLabel className="font-body text-tx-3">{t('docs.where', { folder })}</DropdownMenuLabel>
        {docs.length === 0 && <p className="px-2 py-1.5 font-sans text-xs text-tx-3">{t('docs.none')}</p>}
        {docs.map((doc) => (
          <DropdownMenuItem key={`${doc.base}:${doc.path}`} onSelect={() => navigate(knowledgeHref(doc.base, doc.path))} className="font-sans text-xs">
            <FileText aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
            <span className="min-w-0 flex-1 truncate">{doc.path.slice(folder.length)}</span>
            {doc.base !== 'org' && <span className="shrink-0 text-tx-3">{t('docs.team')}</span>}
          </DropdownMenuItem>
        ))}
        {!hasRunbook && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem disabled={start.isPending || org.data?.canEdit === false} onSelect={() => start.mutate()} className="font-sans text-xs">
              <FilePlus2 aria-hidden className="h-3.5 w-3.5 shrink-0" />
              {t('docs.start_runbook')}
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
