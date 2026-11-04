import { Check, Link2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumRow } from '@/api/rum';
import { writeClipboardText } from '@/lib/clipboard';
import { ChromeButton } from '@/shell/chrome';

import { sessionPath } from './moment';
import { type ReplayController, replayClockMicros, useReplaySelector } from '../replay/controller';

/**
 * Copies the address of the session at the moment the replay is at, so that whoever opens it sees what
 * was meant. Where there is no replay, or it has not been read yet, it is the address of the session.
 */
export function CopyMomentLink({
  id,
  session,
  controller,
}: {
  id: string;
  session: RumRow;
  controller: ReplayController;
}) {
  const { t } = useTranslation('rum');
  const known = useReplaySelector(controller, (state) => state.origin !== null);
  const [copied, setCopied] = React.useState(false);
  const timer = React.useRef<number | undefined>(undefined);

  React.useEffect(() => () => window.clearTimeout(timer.current), []);

  const copy = async () => {
    const path = sessionPath(id, { session, at: replayClockMicros(controller.getState()) });

    try {
      await writeClipboardText(`${window.location.origin}${path}`);
      setCopied(true);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopied(false), 1_500);
    } catch {
      // Clipboard access can be denied by the browser.
    }
  };

  return (
    <ChromeButton data-rum-copy-link="" onClick={() => void copy()}>
      {copied ? <Check aria-hidden className="h-3.5 w-3.5" /> : <Link2 aria-hidden className="h-3.5 w-3.5" />}
      <span aria-live="polite">{copied ? t('playlists.link.copied') : t(known ? 'playlists.link.moment' : 'playlists.link.session')}</span>
    </ChromeButton>
  );
}
