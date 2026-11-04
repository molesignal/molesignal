import { uiSectionTitleClass } from '@/shell/chrome';

/** The title of one part of the insights, a line saying what it shows, and an optional figure beside them. */
export function SectionHeading({
  id,
  title,
  description,
  aside,
}: {
  id: string;
  title: string;
  description: string;
  aside?: string;
}) {
  return (
    <div className="flex flex-col gap-1 sm:flex-row sm:items-start sm:justify-between sm:gap-6">
      <div>
        <h3 id={id} className={uiSectionTitleClass}>
          {title}
        </h3>
        <p className="mt-0.5 font-sans text-xs text-tx-2">{description}</p>
      </div>
      {aside && <p className="shrink-0 font-sans text-xs tabular-nums text-tx-2">{aside}</p>}
    </div>
  );
}
