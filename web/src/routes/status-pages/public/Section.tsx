/** The heading of one part of a public status page, with the window it speaks of beside it. */
export function SectionHeading({ id, title, meta }: { id: string; title: string; meta?: string }) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <h2 id={id} className="text-xl font-display-strong tracking-[-0.015em] text-tx-0">
        {title}
      </h2>
      {meta && <span className="shrink-0 text-sm text-tx-2">{meta}</span>}
    </div>
  );
}
