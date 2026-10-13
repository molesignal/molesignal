import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Languages } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import * as meApi from '@/api/me';
import { toApiError } from '@/lib/http';
import {
  USER_PREFERENCES_QUERY_KEY,
  useApplyUserPreferences,
} from '@/shell/PreferenceRuntime';
import {
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import { useThemeStore } from '@/stores/useThemeStore';

/** The languages the product is read in. Each is named in itself, whatever language is on screen. */
const LANGUAGES: readonly meApi.PreferenceLanguage[] = ['zh-cn', 'en-us'];

/**
 * Lists in which the backend names what the product ships with (built-in prompts, built-in
 * functions) in the reader's language. They are read again once the new language is saved,
 * not before: the backend goes by the saved one.
 */
const NAMED_BY_BACKEND: ReadonlyArray<readonly string[]> = [['agent', 'prompts'], ['functions']];

/**
 * The language of the product, in the user menu: the one row of the menu that
 * changes something at once. Picking a language switches the page to it and
 * keeps it with the reader's other preferences, so it is the same in another
 * browser. A language that could not be kept is put back, with the reason.
 */
export function UserMenuLanguage() {
  const { t } = useTranslation('shell');
  const queryClient = useQueryClient();
  const applyUserPreferences = useApplyUserPreferences();
  const language = useThemeStore((state) => state.language);
  const setLanguage = useThemeStore((state) => state.setLanguage);

  const change = useMutation({
    mutationFn: async (next: meApi.PreferenceLanguage) => {
      const current =
        queryClient.getQueryData<meApi.UserPreferences>(USER_PREFERENCES_QUERY_KEY)
        ?? (await meApi.preferences());

      return meApi.updatePreferences({ ...current, language: next });
    },
    onMutate: async (next) => {
      await queryClient.cancelQueries({ queryKey: USER_PREFERENCES_QUERY_KEY });
      const previous = queryClient.getQueryData<meApi.UserPreferences>(USER_PREFERENCES_QUERY_KEY);
      const previousLanguage = language;

      // At once: the menu the reader is looking at is the first thing to change.
      setLanguage(next);
      if (previous) queryClient.setQueryData(USER_PREFERENCES_QUERY_KEY, { ...previous, language: next });

      return { previous, previousLanguage };
    },
    onSuccess: (saved) => {
      queryClient.setQueryData(USER_PREFERENCES_QUERY_KEY, saved);
      applyUserPreferences(saved);
      for (const queryKey of NAMED_BY_BACKEND) void queryClient.invalidateQueries({ queryKey });
    },
    onError: (error, _next, context) => {
      if (context?.previous) queryClient.setQueryData(USER_PREFERENCES_QUERY_KEY, context.previous);
      if (context) setLanguage(context.previousLanguage);
      toast.error(toApiError(error).message);
    },
  });

  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger data-ui="language-trigger" className="min-h-10 rounded-md px-3">
        <Languages className="h-4 w-4 text-tx-2" />
        <span className="min-w-0 flex-1 truncate">{t('user_menu.language')}</span>
        <span data-ui="language-current" className="shrink-0 font-sans text-xs text-tx-3">
          {t(`user_menu.languages.${language}`)}
        </span>
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent data-ui="language-menu" className="w-44 shadow-none" sideOffset={12}>
        <DropdownMenuRadioGroup
          value={language}
          onValueChange={(next) => {
            if (next !== language) change.mutate(next as meApi.PreferenceLanguage);
          }}
        >
          {LANGUAGES.map((option) => (
            <DropdownMenuRadioItem
              key={option}
              value={option}
              lang={option === 'zh-cn' ? 'zh-CN' : 'en-US'}
              className="min-h-10 rounded-md"
            >
              {t(`user_menu.languages.${option}`)}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  );
}
