import type { TFunction } from 'i18next';
import { Check, Copy } from 'lucide-react';
import * as React from 'react';

import * as statusPagesApi from '@/api/statusPages';
import { writeClipboardText } from '@/lib/clipboard';
import { cn } from '@/shell/lib/cn';
import { IconSwap } from '@/shell/polish/IconSwap';
import { Button } from '@/shell/ui/button';
import { Input } from '@/shell/ui/input';

import { PUBLIC_CONTROL_HEIGHT } from '../control';

/**
 * The page as a feed, in RSS and in Atom: an address to put in a reader, with
 * nothing to confirm. A feed carries every event of the page.
 */
export function FeedAddresses({ slug, copy }: { slug: string; copy: TFunction }) {
  return (
    <div className="space-y-4">
      <FeedAddress label={copy('public.subscription.feed_rss')} url={absolute(statusPagesApi.publicRssUrl(slug))} copy={copy} />
      <FeedAddress label={copy('public.subscription.feed_atom')} url={absolute(statusPagesApi.publicAtomUrl(slug))} copy={copy} />
      <p className="text-xs leading-5 text-tx-3">{copy('public.subscription.feed_hint')}</p>
    </div>
  );
}

function FeedAddress({ label, url, copy }: { label: string; url: string; copy: TFunction }) {
  const inputId = React.useId();
  const [copied, setCopied] = React.useState(false);
  const resetTimer = React.useRef<number | undefined>(undefined);

  React.useEffect(() => () => window.clearTimeout(resetTimer.current), []);

  const copyAddress = async () => {
    try {
      await writeClipboardText(url);
      setCopied(true);
      window.clearTimeout(resetTimer.current);
      resetTimer.current = window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // The browser can refuse the clipboard; the address can still be selected by hand.
    }
  };

  return (
    <div>
      <label htmlFor={inputId} className="block text-sm font-strong text-tx-1">
        {label}
      </label>
      <div className="mt-2 flex gap-2">
        <Input
          id={inputId}
          readOnly
          value={url}
          onFocus={(event) => event.currentTarget.select()}
          className={cn(PUBLIC_CONTROL_HEIGHT, 'min-w-0 flex-1 text-base sm:text-sm')}
        />
        <Button
          type="button"
          variant="outline"
          aria-label={copy(copied ? 'public.subscription.feed_copied' : 'public.subscription.feed_copy', { name: label })}
          className={cn(PUBLIC_CONTROL_HEIGHT, 'aspect-square shrink-0 px-0')}
          onClick={() => void copyAddress()}
        >
          <IconSwap
            active={copied}
            inactiveIcon={<Copy aria-hidden className="h-4 w-4" />}
            activeIcon={<Check aria-hidden className="h-4 w-4" />}
          />
        </Button>
      </div>
    </div>
  );
}

/** A reader is given the whole address, on whatever host the page is read. */
function absolute(path: string): string {
  return new URL(path, window.location.origin).toString();
}
