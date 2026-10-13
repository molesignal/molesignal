import { MOLESIGNAL_WEBSITE } from '@/product/website';
import { cn } from '@/shell/lib/cn';
import { LogoMark } from '@/shell/LogoMark';

/**
 * The line that says what serves a public status page, linked to MoleSignal's
 * website. It stands at the foot of every public view: the page itself, its
 * history, an incident, and the frames shown before a page can be read (a
 * private page's door, a page that is not there).
 *
 * The page belongs to whoever publishes it, so the credit is the quietest
 * thing on it: the mark, one line, and nothing of the publisher's colour.
 */
export function PublicStatusCredit({ label, className }: { label: string; className?: string }) {
  return (
    <a
      href={MOLESIGNAL_WEBSITE}
      target="_blank"
      rel="noopener noreferrer"
      data-status-page-credit
      className={cn(
        'group inline-flex min-h-[44px] items-center gap-2 rounded-md text-sm text-tx-2 transition-colors hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0 md:min-h-0',
        className,
      )}
    >
      <LogoMark size={16} className="shrink-0" />
      <span className="underline-offset-4 group-hover:underline group-focus-visible:underline">{label}</span>
    </a>
  );
}
