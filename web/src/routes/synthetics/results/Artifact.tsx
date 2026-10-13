import { Download } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { getResultArtifact, type SyntheticResultArtifact } from '@/api/synthetics';
import { ChromeButton } from '@/shell/chrome';

/**
 * Something a run left behind: a screenshot, a HAR, a trace. It is fetched only
 * when asked for, since most runs are opened for their error and not for these.
 */
export function ArtifactPreview({
  resultId,
  artifact,
}: {
  resultId: string;
  artifact: SyntheticResultArtifact;
}) {
  const { t } = useTranslation('synthetics');
  const [url, setUrl] = React.useState<string>();
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState(false);
  const mounted = React.useRef(true);
  const urlRef = React.useRef<string>();

  React.useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    };
  }, []);

  const load = async (): Promise<string | undefined> => {
    if (urlRef.current) return urlRef.current;
    setLoading(true);
    setError(false);
    try {
      const blob = await getResultArtifact(resultId, artifact.id);
      if (!mounted.current) return undefined;
      const objectUrl = URL.createObjectURL(blob);
      urlRef.current = objectUrl;
      setUrl(objectUrl);
      return objectUrl;
    } catch {
      if (mounted.current) setError(true);
      return undefined;
    } finally {
      if (mounted.current) setLoading(false);
    }
  };

  const download = async () => {
    const artifactUrl = await load();
    if (!artifactUrl) return;
    const anchor = document.createElement('a');
    anchor.href = artifactUrl;
    anchor.download = fileName(artifact);
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
  };
  const screenshot = artifact.kind === 'screenshot';

  return (
    <li data-artifact={artifact.kind} className="py-[10px]">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="truncate font-sans text-sm font-strong text-tx-0">
            {artifact.name || artifact.kind}
          </div>
          <div className="mt-[2px] font-sans text-xs text-tx-2">
            {artifact.kind} · {formatBytes(artifact.content_length)}
          </div>
        </div>
        {screenshot && !url && (
          <ChromeButton size="sm" disabled={loading} onClick={() => void load()}>
            {t('detail.load_screenshot')}
          </ChromeButton>
        )}
        <ChromeButton
          size="sm"
          disabled={loading}
          aria-label={`${t('detail.download_artifact')}: ${artifact.name || artifact.kind}`}
          onClick={() => void download()}
        >
          <Download aria-hidden className="h-3.5 w-3.5" />
          {t('detail.download')}
        </ChromeButton>
      </div>
      {screenshot && url && (
        <img
          src={url}
          alt={t('detail.screenshot_alt', { name: artifact.name })}
          className="mt-[10px] max-h-[520px] w-full rounded-md bg-bg-2 object-contain"
        />
      )}
      {loading && <p className="mt-[6px] font-sans text-xs text-tx-2">{t('detail.loading_artifact')}</p>}
      {error && (
        <p role="alert" className="mt-[6px] font-sans text-xs text-red">
          {t('detail.artifact_unavailable')}
        </p>
      )}
    </li>
  );
}

function fileName(artifact: SyntheticResultArtifact): string {
  if (artifact.kind === 'screenshot') return `${artifact.name || 'screenshot'}.png`;
  if (artifact.kind === 'har') return `${artifact.name || 'journey'}.har`;
  return `${artifact.name || 'journey'}.trace.json`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}
