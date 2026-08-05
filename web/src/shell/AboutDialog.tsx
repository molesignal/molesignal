import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as versionApi from '@/api/version';
import { tzOffsetLabel, useTimeFormatter } from '@/lib/time';
import { ChromeButton } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';
import { LogoMark } from '@/shell/LogoMark';
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

const EDITION_TRANSLATION_KEYS = {
  community: 'about.editions.community',
  enterprise: 'about.editions.enterprise',
  oss: 'about.editions.oss',
  pro: 'about.editions.pro',
  saas: 'about.editions.saas',
} as const;

function formatBuildTimeUtc(epochSecs: number | undefined): string {
  if (!epochSecs) return '—';
  return `${new Date(epochSecs * 1000).toISOString().slice(0, 19).replace('T', ' ')} UTC`;
}

function humanizeEdition(edition: string): string {
  return edition
    .trim()
    .replace(/[-_]+/g, ' ')
    .replace(/\b\w/g, (character) => character.toUpperCase());
}

export function shouldShowBuildBranch(
  version: versionApi.VersionInfo | undefined,
  isDevelopmentBuild = import.meta.env.DEV,
): boolean {
  if (!version?.branch) return false;
  if (isDevelopmentBuild) return true;
  return /(?:dev|develop|test|staging|preview|alpha|beta|rc|snapshot|canary)/i.test(
    `${version.version} ${version.branch}`,
  );
}

function useClipboardFeedback() {
  const [copied, setCopied] = React.useState(false);
  const resetTimer = React.useRef<number | null>(null);

  React.useEffect(
    () => () => {
      if (resetTimer.current !== null) window.clearTimeout(resetTimer.current);
    },
    [],
  );

  const copy = React.useCallback(async (value: string) => {
    if (!value || !navigator.clipboard?.writeText) return;
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      return;
    }
    setCopied(true);
    if (resetTimer.current !== null) window.clearTimeout(resetTimer.current);
    resetTimer.current = window.setTimeout(() => setCopied(false), 1600);
  }, []);

  return { copied, copy };
}

/** "About" dialog — surfaces the backend build/version info under the ? menu. */
export function AboutDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const { t } = useTranslation(['shell', 'common']);
  const diagnosticsCopy = useClipboardFeedback();
  const timeFormatter = useTimeFormatter();
  const q = useQuery({
    queryKey: ['version'],
    queryFn: () => versionApi.get(),
    enabled: open,
    staleTime: 60_000,
  });
  const v = q.data;
  const dash = q.isError ? '—' : '…';
  const editionKey = v
    ? EDITION_TRANSLATION_KEYS[
        v.edition.trim().toLowerCase() as keyof typeof EDITION_TRANSLATION_KEYS
      ]
    : undefined;
  const edition = v
    ? editionKey
      ? t(editionKey)
      : humanizeEdition(v.edition)
    : dash;
  const buildTimeUtc = v ? formatBuildTimeUtc(v.build_epoch_secs) : dash;
  const buildTime = v
    ? t('about.time_with_zone', {
        time: timeFormatter.millis(v.build_epoch_secs * 1000),
        zone: tzOffsetLabel(timeFormatter.tz),
      })
    : dash;
  const shortCommit = v?.commit ? v.commit.slice(0, 7) : dash;
  const showBranch = shouldShowBuildBranch(v);
  const diagnostics = v
    ? [
        'MoleSignal',
        `${t('about.version')}: v${v.version}`,
        `${t('about.edition')}: ${edition} (${v.edition})`,
        `${t('about.release_channel')}: ${v.release_channel}`,
        `${t('about.commit')}: ${v.commit}`,
        `${t('about.build_id')}: ${v.build_id}`,
        `${t('about.built')}: ${buildTimeUtc}`,
        ...(v.branch ? [`${t('about.branch')}: ${v.branch}`] : []),
      ].join('\n')
    : '';

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[calc(100vh-2rem)] w-[calc(100vw-2rem)] max-w-[540px] gap-0 overflow-y-auto p-0 sm:rounded-xl">
        <DialogHeader className="px-6 pb-0 pt-4 pr-12">
          <DialogTitle
            aria-label={t('about.accessible_title')}
            className="text-base leading-6"
          >
            {t('about.title')}
          </DialogTitle>
        </DialogHeader>
        <div className="px-6 pb-4 pt-3">
          <div className="flex items-start gap-3">
            <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-bg-2">
              <LogoMark size={30} />
            </div>
            <div className="min-w-0 pt-0.5">
              <div className="font-sans text-xl font-semibold leading-6 text-tx-0">
                MoleSignal
              </div>
              <DialogDescription className="mt-1 max-w-[390px] font-sans text-sm leading-5 text-tx-2">
                {t('about.subtitle')}
              </DialogDescription>
            </div>
          </div>

          <dl className="mt-4">
            <InfoRow
              label={t('about.version')}
              value={v ? `v${v.version}` : dash}
              code
              testId="about-version"
            />
            <InfoRow
              label={t('about.edition')}
              value={edition}
              testId="about-edition"
            />
            <InfoRow
              label={t('about.release_channel')}
              value={v?.release_channel ?? dash}
              code
              testId="about-release-channel"
            />
          </dl>

          <div className="mt-2">
            <dl className="pb-1">
              <InfoRow
                label={t('about.commit')}
                value={shortCommit}
                code
                testId="about-commit"
              />
              <InfoRow
                label={t('about.build_id')}
                value={v?.build_id ?? dash}
                code
                testId="about-build-id"
              />
              <InfoRow
                label={t('about.built')}
                value={buildTime}
                tooltip={buildTimeUtc}
                testId="about-build-time"
                tabular
              />
              {showBranch && (
                <InfoRow
                  label={t('about.branch')}
                  value={v?.branch ?? dash}
                  code
                  testId="about-branch"
                />
              )}
            </dl>
          </div>
        </div>

        <DialogFooter className="items-center justify-between space-x-3 border-t border-bd-0 bg-bg-1 px-6 py-2.5">
          <CopyIconButton
            disabled={!v}
            onClick={() => void diagnosticsCopy.copy(diagnostics)}
            label={t('about.copy_diagnostics')}
            copied={diagnosticsCopy.copied}
            copiedLabel={t('about.diagnostics_copied')}
          />
          <DialogClose asChild>
            <ChromeButton size="sm" data-testid="about-close">
              {t('common:actions.close')}
            </ChromeButton>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function InfoRow({
  label,
  value,
  code,
  tabular,
  tooltip,
  testId,
}: {
  label: string;
  value: string;
  code?: boolean;
  tabular?: boolean;
  tooltip?: string;
  testId?: string;
}) {
  const valueNode = (
    <span
      data-testid={testId}
      tabIndex={tooltip ? 0 : undefined}
      className={cn(
        'min-w-0 select-all break-all font-sans text-sm text-tx-1',
        code && 'font-mono text-[13px] tabular-nums',
        tabular && 'tabular-nums',
        tooltip && 'cursor-help',
      )}
    >
      {value}
    </span>
  );

  return (
    <div className="grid min-h-11 grid-cols-[116px_minmax(0,1fr)] items-center gap-4">
      <dt className="font-sans text-[13px] text-tx-3">{label}</dt>
      <dd className="flex min-w-0 items-center gap-2">
        {tooltip ? (
          <Tooltip>
            <TooltipTrigger asChild>{valueNode}</TooltipTrigger>
            <TooltipContent side="top">{tooltip}</TooltipContent>
          </Tooltip>
        ) : (
          valueNode
        )}
      </dd>
    </div>
  );
}
