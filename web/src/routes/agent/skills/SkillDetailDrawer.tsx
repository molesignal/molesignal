import { useQuery } from '@tanstack/react-query';
import { Download, FileText, MessageSquarePlus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as skillsApi from '@/api/agent/skills';
import { toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { FormDrawer, FormSection } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';

/** Saves a downloaded package where the browser puts downloads. */
async function download(id: string) {
  const { blob, fileName } = await skillsApi.downloadSkillPackage(id);
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = fileName;
  link.click();
  URL.revokeObjectURL(url);
}

/**
 * What a skill holds: its instructions, its files (text ones can be read
 * here), the tools it asks for and how to call it. It can be downloaded and
 * tried in the chat.
 */
export function SkillDetailDrawer({
  skill,
  onClose,
}: {
  skill: skillsApi.Skill | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('agent');
  const openAgent = useMoleAgentStore((state) => state.open);
  const [file, setFile] = React.useState<string | null>(null);
  const detail = useQuery({
    queryKey: ['agent', 'skill', skill?.id],
    queryFn: () => skillsApi.getSkill(skill?.id ?? ''),
    enabled: Boolean(skill),
    retry: false,
  });
  const content = useQuery({
    queryKey: ['agent', 'skill-file', skill?.id, file],
    queryFn: () => skillsApi.getSkillFile(skill?.id ?? '', file ?? ''),
    enabled: Boolean(skill && file),
    retry: false,
  });

  React.useEffect(() => setFile(null), [skill]);

  if (!skill) return null;
  const data = detail.data;

  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={skill.title}
      subtitle={skill.description}
      width={720}
      footer={
        <>
          <Button
            variant="outline"
            onClick={() => {
              download(skill.id).catch((error: unknown) =>
                toast.error(toApiError(error).message),
              );
            }}
          >
            <Download className="h-3.5 w-3.5" />
            {t('settings.skills.detail.download')}
          </Button>
          <Button
            disabled={!skill.enabled || skill.requires_runner}
            onClick={() => {
              openAgent(`/${skill.name} `);
              onClose();
            }}
          >
            <MessageSquarePlus className="h-3.5 w-3.5" />
            {t('settings.skills.detail.try')}
          </Button>
        </>
      }
    >
      {detail.isLoading ? (
        <ProductState variant="loading" compact />
      ) : detail.isError ? (
        <ProductState variant="error" error={detail.error} compact />
      ) : data ? (
        <>
          <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1.5 text-xs">
            <dt className="text-tx-3">{t('settings.skills.detail.invoke')}</dt>
            <dd className="font-mono text-tx-0">
              /{data.name}
              {data.argument_hint ? ` ${data.argument_hint}` : ''}
            </dd>
            {data.version && (
              <>
                <dt className="text-tx-3">{t('settings.skills.detail.version')}</dt>
                <dd className="text-tx-1">{data.version}</dd>
              </>
            )}
            <dt className="text-tx-3">{t('settings.skills.detail.hash')}</dt>
            <dd className="font-mono text-tx-2">{data.hash.slice(0, 12)}</dd>
            {data.allowed_tools.length > 0 && (
              <>
                <dt className="text-tx-3">{t('settings.skills.detail.allowed_tools')}</dt>
                <dd className="font-mono text-tx-1">{data.allowed_tools.join(', ')}</dd>
              </>
            )}
            {data.dir && (
              <>
                <dt className="text-tx-3">{t('settings.skills.detail.dir')}</dt>
                <dd className="font-mono text-tx-2">{data.dir}</dd>
              </>
            )}
          </dl>
          {data.requires_runner && (
            <p className="mt-3 rounded-md bg-[var(--yellow-tint)] p-3 text-xs leading-5 text-[var(--yellow-ink)]">
              {t('settings.skills.detail.needs_runner')}
            </p>
          )}
          {data.examples.length > 0 && (
            <FormSection title={t('settings.skills.detail.examples')} className="mt-5">
              <ul className="space-y-1 text-xs text-tx-1">
                {data.examples.map((example) => (
                  <li key={example} className="rounded-md bg-bg-2 px-3 py-1.5">
                    {example}
                  </li>
                ))}
              </ul>
            </FormSection>
          )}
          <FormSection title={t('settings.skills.detail.instructions')} className="mt-5">
            <pre className="max-h-80 overflow-auto whitespace-pre-wrap rounded-md bg-bg-2 p-3 font-sans text-xs leading-5 text-tx-1">
              {data.body}
            </pre>
          </FormSection>
          <FormSection
            title={t('settings.skills.detail.files', { count: data.files.length })}
            className="mt-5"
          >
            <ul className="divide-y divide-bd-0 overflow-hidden rounded-md bg-bg-2">
              {data.files.map((entry) => (
                <li key={entry.path}>
                  <button
                    type="button"
                    disabled={!entry.text}
                    onClick={() => setFile(entry.path === file ? null : entry.path)}
                    className={cn(
                      'flex w-full items-center gap-2 px-3 py-2 text-left text-xs',
                      entry.text ? 'hover:bg-bg-3' : 'cursor-default text-tx-3',
                      entry.path === file && 'bg-bg-3',
                    )}
                  >
                    <FileText className="h-3.5 w-3.5 shrink-0 text-tx-3" />
                    <span className="min-w-0 flex-1 truncate font-mono text-tx-1">{entry.path}</span>
                    <span className="shrink-0 text-tx-3">{formatSize(entry.size)}</span>
                  </button>
                  {entry.path === file && (
                    <pre className="max-h-72 overflow-auto whitespace-pre-wrap bg-bg-1 px-3 py-2 font-sans text-xs leading-5 text-tx-1">
                      {content.isLoading ? '…' : content.isError ? toApiError(content.error).message : content.data}
                    </pre>
                  )}
                </li>
              ))}
            </ul>
          </FormSection>
        </>
      ) : null}
    </FormDrawer>
  );
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}
