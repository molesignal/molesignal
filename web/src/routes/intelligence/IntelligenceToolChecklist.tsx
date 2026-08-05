export function IntelligenceToolChecklist({
  options,
  selected,
  onChange,
}: {
  options: Array<{ value: string; label: string; hint?: string }>;
  selected: string[];
  onChange: (selected: string[]) => void;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      {options.map((option) => {
        const checked = selected.includes(option.value);
        return (
          <label
            key={option.value}
            className={
              checked
                ? 'flex min-h-11 cursor-pointer items-start gap-3 rounded-md border border-indigo/45 bg-bg-2 px-3 py-2.5'
                : 'flex min-h-11 cursor-pointer items-start gap-3 rounded-md border border-bd-0 bg-bg-1 px-3 py-2.5 hover:border-bd-2 hover:bg-bg-2'
            }
          >
            <input
              type="checkbox"
              checked={checked}
              onChange={(event) =>
                onChange(
                  event.target.checked
                    ? [...selected, option.value]
                    : selected.filter((value) => value !== option.value),
                )
              }
              className="mt-0.5 h-4 w-4 accent-indigo"
            />
            <span className="min-w-0 flex-1">
              <span className="block text-sm font-semibold text-tx-0">
                {option.label}
              </span>
              {option.hint && (
                <span className="mt-1 block text-xs text-tx-3">
                  {option.hint}
                </span>
              )}
            </span>
          </label>
        );
      })}
    </div>
  );
}
