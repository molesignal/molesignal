import type { TFunction } from 'i18next';

import type { PublicStatusPageComponent } from '@/api/statusPages';
import { cn } from '@/shell/lib/cn';
import { Checkbox } from '@/shell/ui/checkbox';

import { componentTree, namedComponents } from '../../model';

/**
 * What a subscriber hears about: everything the page says, or only what
 * names the components they rely on. A page of one component has nothing to
 * choose between, so it asks nothing. A group is a heading over the
 * components in it: what is chosen is the components.
 */
export function ComponentChoice({
  components,
  scope,
  chosen,
  onScopeChange,
  onChosenChange,
  copy,
  disabled,
}: {
  components: PublicStatusPageComponent[];
  scope: 'all' | 'selected';
  chosen: string[];
  onScopeChange: (scope: 'all' | 'selected') => void;
  onChosenChange: (chosen: string[]) => void;
  copy: TFunction;
  disabled?: boolean;
}) {
  if (namedComponents(components).length < 2) return null;

  const choice = (component: PublicStatusPageComponent) => {
    const checked = chosen.includes(component.id);
    return (
      <li key={component.id}>
        <label className="flex min-h-[40px] cursor-pointer items-center gap-3 rounded px-2 text-sm text-tx-1 hover:bg-bg-2">
          <Checkbox
            checked={checked}
            onCheckedChange={(next) =>
              onChosenChange(
                next === true
                  ? [...chosen, component.id]
                  : chosen.filter((id) => id !== component.id),
              )}
            className="data-[state=checked]:border-[var(--status-brand)] data-[state=checked]:bg-[var(--status-brand)]"
          />
          <span className="min-w-0 truncate">{component.name}</span>
        </label>
      </li>
    );
  };

  return (
    <fieldset className="space-y-2" disabled={disabled}>
      <legend className="text-sm font-strong text-tx-1">{copy('public.subscription.components_label')}</legend>
      <div className="grid grid-cols-2 gap-2">
        {(['all', 'selected'] as const).map((option) => (
          <button
            key={option}
            type="button"
            aria-pressed={scope === option}
            onClick={() => onScopeChange(option)}
            className={cn(
              'min-h-[44px] rounded-md px-3 text-sm font-strong transition-colors sm:min-h-10',
              scope === option
                ? 'bg-[var(--status-brand)] text-white focus-visible:brightness-95'
                : 'bg-bg-1 text-tx-1 hover:bg-bg-2 focus-visible:bg-bg-2',
            )}
          >
            {copy(`public.subscription.components_${option}`)}
          </button>
        ))}
      </div>
      {scope === 'selected' && (
        <ul className="max-h-56 space-y-1 overflow-y-auto rounded-md bg-bg-1 p-2">
          {componentTree(components).map(({ item, members }) =>
            item.kind === 'group' ? (
              members.length > 0 && (
                <li key={item.id}>
                  <p className="px-2 pb-0.5 pt-2 text-xs font-strong text-tx-3">{item.name}</p>
                  <ul className="space-y-1 pl-3">{members.map(choice)}</ul>
                </li>
              )
            ) : (
              choice(item)
            ))}
        </ul>
      )}
      <p className="text-xs leading-5 text-tx-3">
        {copy(scope === 'all' ? 'public.subscription.components_all_hint' : 'public.subscription.components_selected_hint')}
      </p>
    </fieldset>
  );
}
