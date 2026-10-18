import { useQuery } from '@tanstack/react-query';
import { ArrowRight, MessageSquareText, Pencil, Plus, Siren } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as agentApi from '@/api/agent';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { Button } from '@/shell/ui/button';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shell/ui/table';

import { InvestigationEditorDrawer, type InvestigationEditorTarget } from './Editors';
import { EmptyTab, ModulePage, StateBadge } from './Pages';

/** The incident an investigation was started from, when it was. */
function incidentOf(investigation: agentApi.Investigation): string | null {
  const id = investigation.context?.incident_id;
  return typeof id === 'string' && id ? id : null;
}

/**
 * The investigations: those a chat started from an incident records (steps,
 * evidence, hypotheses and the conclusion, written by Mole Agent as it works),
 * and those written by hand. Each leads to its record, to the chat that wrote
 * it, and to the incident it is about.
 */
export function InvestigationsPage() {
  const { t } = useTranslation('agent');
  const [editor, setEditor] = React.useState<InvestigationEditorTarget>(null);
  const investigations = useQuery({
    queryKey: ['agent', 'investigations'],
    queryFn: agentApi.listInvestigations,
    retry: false,
  });

  return (
    <ModulePage
      title={t('investigations.title')}
      description={t('investigations.description')}
      empty={investigations.isSuccess && investigations.data.length === 0}
      flush={Boolean(investigations.data?.length)}
      action={
        <Button size="sm" onClick={() => setEditor('new')}>
          <Plus /> {t('investigations.create')}
        </Button>
      }
    >
      {investigations.isLoading ? (
        <ProductState variant="loading" />
      ) : investigations.isError ? (
        <ProductState variant="error" error={investigations.error} />
      ) : investigations.data?.length ? (
        // The first and the last column keep the gutter the line over the table has.
        <div
          data-agent-list
          className="overflow-hidden [&_td:first-child]:pl-5 [&_td:last-child]:pr-5 [&_th:first-child]:pl-5 [&_th:last-child]:pr-5"
        >
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('investigations.columns.task')}</TableHead>
                <TableHead>{t('investigations.columns.source')}</TableHead>
                <TableHead>{t('common.status')}</TableHead>
                <TableHead>{t('investigations.columns.current_step')}</TableHead>
                <TableHead>{t('common.updated')}</TableHead>
                <TableHead className="w-12" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {investigations.data.map((item) => {
                const incident = incidentOf(item);
                return (
                  <TableRow key={item.id} data-agent-investigation={item.id}>
                    <TableCell>
                      <Link
                        to={`/agent/investigations/${encodeURIComponent(item.id)}`}
                        className="font-strong text-tx-0 hover:text-indigo"
                      >
                        {item.title}
                      </Link>
                      {item.summary && <p className="mt-0.5 max-w-xl truncate text-xs text-tx-3">{item.summary}</p>}
                    </TableCell>
                    <TableCell className="whitespace-nowrap text-xs">
                      {incident ? (
                        <Link
                          to={`/alerts/incidents/${encodeURIComponent(incident)}`}
                          className="inline-flex items-center gap-1 text-indigo hover:underline"
                        >
                          <Siren className="h-3.5 w-3.5" />
                          {t('investigations.from_incident')}
                        </Link>
                      ) : (
                        <span className="text-tx-3">{t('investigations.by_hand')}</span>
                      )}
                    </TableCell>
                    <TableCell><StateBadge value={item.status} /></TableCell>
                    <TableCell className="max-w-xs truncate text-tx-2">{item.current_step ?? '—'}</TableCell>
                    <TableCell className="whitespace-nowrap text-xs text-tx-3">
                      {formatMicrosActive(item.updated_at)}
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center justify-end">
                        {item.chat_id && (
                          <Button variant="ghost" size="icon" asChild aria-label={t('investigations.open_chat')}>
                            <Link to={`/agent/chat?chat=${encodeURIComponent(item.chat_id)}`}>
                              <MessageSquareText />
                            </Link>
                          </Button>
                        )}
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={t('investigations.edit')}
                          onClick={() => setEditor(item)}
                        >
                          <Pencil />
                        </Button>
                        <Button variant="ghost" size="icon" asChild aria-label={t('common.open')}>
                          <Link to={`/agent/investigations/${encodeURIComponent(item.id)}`}>
                            <ArrowRight />
                          </Link>
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        </div>
      ) : (
        <EmptyTab
          title={t('investigations.empty_title')}
          purpose={t('investigations.description')}
          hint={t('investigations.empty_description')}
          action={<Button onClick={() => setEditor('new')}>{t('investigations.create')}</Button>}
        />
      )}
      <InvestigationEditorDrawer
        target={editor}
        onClose={() => setEditor(null)}
      />
    </ModulePage>
  );
}
