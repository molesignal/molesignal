import type { TFunction } from 'i18next';
import type { ReactNode } from 'react';

import { cn } from '@/shell/lib/cn';
import {
  floatingMenuContentClass,
  floatingMenuItemClass,
  floatingMenuLabelClass,
} from '@/shell/ui/floating';

import { groupCommands } from './commands';
import type { CommandMenuState } from './menu';

/** A menu over the chat stands off the text under it: the large shadow, with its hairline. */
export const OVER_CHAT = '!shadow-lg';

export function optionId(menuId: string, index: number): string {
  return `${menuId}-option-${index}`;
}

/**
 * The menu over the composer while a command is typed: the commands that match,
 * then the suggestions for its argument. The composer keeps the focus; the
 * highlighted row is its active descendant.
 */
export function CommandMenu({
  t,
  id,
  menu,
  active,
  onActivate,
  onChoose,
}: {
  t: TFunction<'agent'>;
  id: string;
  menu: CommandMenuState;
  active: number;
  onActivate: (index: number) => void;
  onChoose: (index: number) => void;
}) {
  const row = (index: number, disabled: boolean, children: ReactNode) => (
    <li
      key={index}
      id={optionId(id, index)}
      role="option"
      aria-selected={index === active}
      aria-disabled={disabled || undefined}
      data-highlighted={index === active ? '' : undefined}
      data-disabled={disabled ? '' : undefined}
      onMouseEnter={() => onActivate(index)}
      onMouseDown={(event) => {
        // The composer keeps the focus and the caret.
        event.preventDefault();
        if (!disabled) onChoose(index);
      }}
      className={cn(floatingMenuItemClass, 'min-w-0')}
    >
      {children}
    </li>
  );

  if (menu.kind === 'hint') {
    return (
      <div
        id={id}
        data-agent-command-menu="hint"
        className={cn(floatingMenuContentClass, OVER_CHAT, 'px-[10px] py-[8px]')}
      >
        <p className="flex min-w-0 flex-wrap items-baseline gap-x-2 font-sans text-xs">
          <span className="font-strong text-tx-0">/{menu.command.name}</span>
          {menu.command.argument_hint && (
            <span className="text-tx-2">{menu.command.argument_hint}</span>
          )}
          <span className="min-w-0 text-tx-3">{menu.command.description}</span>
        </p>
      </div>
    );
  }

  if (menu.kind === 'arguments') {
    return (
      <div id={id} data-agent-command-menu="arguments" className={cn(floatingMenuContentClass, OVER_CHAT)}>
        <p className={floatingMenuLabelClass}>
          /{menu.command.name} · {menu.command.description}
        </p>
        <ul role="listbox" aria-label={`/${menu.command.name}`} className="max-h-64 overflow-y-auto">
          {menu.options.map((option, index) =>
            row(
              index,
              false,
              <>
                <span className="shrink-0 font-strong text-tx-0">{option.value}</span>
                {option.label && option.label !== option.value && (
                  <span className="min-w-0 truncate text-tx-2">{option.label}</span>
                )}
                {option.detail && (
                  <span className="ml-auto shrink-0 pl-2 text-tx-3">{option.detail}</span>
                )}
              </>,
            ),
          )}
        </ul>
      </div>
    );
  }

  const groups = groupCommands(menu.commands);
  let index = -1;
  return (
    <div id={id} data-agent-command-menu="commands" className={cn(floatingMenuContentClass, OVER_CHAT)}>
      <ul role="listbox" aria-label={t('commands.menu_label')} className="max-h-72 overflow-y-auto">
        {groups.map((group) => (
          <li key={group.source} role="presentation">
            {groups.length > 1 && (
              <p className={floatingMenuLabelClass}>{t(`commands.groups.${group.source}`)}</p>
            )}
            <ul role="presentation">
              {group.commands.map((command) => {
                index += 1;
                return row(
                  index,
                  !command.available,
                  <>
                    <span className="shrink-0 font-strong text-tx-0">/{command.name}</span>
                    {command.argument_hint && (
                      <span className="hidden shrink-0 text-tx-3 sm:inline">
                        {command.argument_hint}
                      </span>
                    )}
                    {/* Whose skill or which server: a team's skill not in the chat yet says it in its name. */}
                    {(command.server ?? (command.name.includes('@') ? null : command.team)) && (
                      <span
                        data-agent-command-origin
                        className="hidden shrink-0 rounded bg-bg-3 px-1 type-micro text-tx-2 sm:inline"
                      >
                        {command.server ?? command.team}
                      </span>
                    )}
                    <span className="ml-auto min-w-0 truncate pl-2 text-right text-tx-2">
                      {command.available
                        ? command.description
                        : (command.unavailable_reason ?? command.description)}
                    </span>
                  </>,
                );
              })}
            </ul>
          </li>
        ))}
      </ul>
    </div>
  );
}
