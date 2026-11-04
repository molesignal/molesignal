import { useTranslation } from 'react-i18next';

import { uiLabelStrongClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { SsoPlatformMark } from '@/shell/SsoPlatformMark';

import { type PlatformChoice, platformGroups, platformKinds } from './presets';

/** The first step of a new provider: the platform, which fills in the rest. */
export function PlatformPicker({ onPick }: { onPick: (choice: PlatformChoice) => void }) {
  const { t, i18n } = useTranslation('settings-admin');
  const groups = platformGroups(i18n.resolvedLanguage ?? i18n.language);
  return (
    <div className="flex flex-col gap-6" data-testid="sso-platform-picker">
      {groups.map((group) => (
        <section key={group.id} aria-labelledby={`sso-platforms-${group.id}`}>
          <h3 id={`sso-platforms-${group.id}`} className={cn('m-0 mb-2.5', uiLabelStrongClass)}>
            {t(`sso_providers.picker.groups.${group.id}`)}
          </h3>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
            {group.choices.map((choice) => (
              <PlatformTile
                key={`${choice.platform}-${choice.kind}`}
                choice={choice}
                onPick={onPick}
              />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

function PlatformTile({
  choice,
  onPick,
}: {
  choice: PlatformChoice;
  onPick: (choice: PlatformChoice) => void;
}) {
  const { t } = useTranslation('settings-admin');
  const generic = choice.platform === 'custom';
  const name = generic
    ? t(`sso_providers.kinds.${choice.kind}`)
    : t(`sso_providers.platforms.${choice.platform}`);
  const detail = generic
    ? t('sso_providers.picker.generic')
    : platformKinds(choice.platform)
        .map((kind) => t(`sso_providers.kinds.${kind}`))
        .join(' · ');
  return (
    <button
      type="button"
      onClick={() => onPick(choice)}
      className="flex min-w-0 cursor-pointer items-center gap-3 rounded-lg bg-bg-2 px-3 py-2.5 text-left"
      data-platform={choice.platform}
      data-kind={choice.kind}
    >
      <span className="flex h-8 w-8 shrink-0 items-center justify-center">
        <SsoPlatformMark platform={choice.platform} kind={choice.kind} size={22} />
      </span>
      <span className="min-w-0">
        <span className="block truncate font-sans text-sm font-strong text-tx-0">{name}</span>
        <span className="block truncate font-sans text-xs text-tx-3">{detail}</span>
      </span>
    </button>
  );
}
