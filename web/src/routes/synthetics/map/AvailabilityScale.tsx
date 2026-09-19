import { availabilityColor, availabilityHistogram } from './model';

export function AvailabilityScale({
  values,
  label,
  ariaLabel,
}: {
  values: number[];
  label: string;
  ariaLabel: string;
}) {
  const bins = availabilityHistogram(values);
  const maximum = Math.max(1, ...bins);
  const gradient = `linear-gradient(90deg, ${availabilityColor(0)}, ${availabilityColor(0.25)}, ${availabilityColor(0.5)}, ${availabilityColor(0.75)}, ${availabilityColor(1)})`;

  return (
    <div
      data-availability-scale
      aria-label={ariaLabel}
      className="w-48 rounded-md border border-bd-0 bg-bg-1/95 px-2.5 py-1.5 font-sans sm:w-56"
    >
      <div className="text-type-micro mb-0.5 flex items-center justify-between leading-3 text-tx-2">
        <span className="font-strong text-tx-1">{label}</span>
        <span>0–100%</span>
      </div>
      <div aria-hidden className="flex h-4 items-end gap-px">
        {bins.map((count, index) => (
          <i
            key={index}
            className="min-w-0 flex-1 rounded-t-[1px]"
            style={{
              height: `${count > 0 ? Math.max(3, Math.round((count / maximum) * 16)) : 1}px`,
              background: availabilityColor((index + 0.5) / bins.length),
              opacity: count > 0 ? 0.9 : 0.25,
            }}
          />
        ))}
      </div>
      <div aria-hidden className="mt-px h-1" style={{ background: gradient }} />
      <div className="text-type-micro mt-px flex justify-between font-sans leading-none text-tx-3">
        <span>0%</span>
        <span>50%</span>
        <span>100%</span>
      </div>
    </div>
  );
}
