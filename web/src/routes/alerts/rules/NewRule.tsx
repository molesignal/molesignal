import { ChevronDown, Plus, SlidersHorizontal, Sparkles } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

/** The way to a new rule, with the choice it starts with: a fixed threshold or a learned baseline. */
export function NewRuleMenu() {
  const { t } = useTranslation('alerts');
  const navigate = useNavigate();
  const manageAccess = useActionAccess({ permission: 'alerts.manage' });

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <ChromeButton variant="primary" disabled={manageAccess.disabled} disabledReason={manageAccess.reason}>
          <Plus className="h-4 w-4" />
          {t('actions.new_rule')}
          <ChevronDown className="h-3.5 w-3.5" />
        </ChromeButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-72">
        <DropdownMenuItem
          className="items-start py-2.5"
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          onSelect={() => navigate('/alerts/rules/new')}
        >
          <SlidersHorizontal className="mt-0.5 h-4 w-4 shrink-0 text-[var(--blue-ink)]" />
          <span className="min-w-0">
            <span className="block font-strong text-tx-0">{t('actions.new_threshold_rule')}</span>
            <span className="mt-0.5 block text-xs leading-relaxed text-tx-2">
              {t('actions.new_threshold_rule_description')}
            </span>
          </span>
        </DropdownMenuItem>
        <DropdownMenuItem
          className="items-start py-2.5"
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          onSelect={() => navigate('/alerts/anomaly/add')}
        >
          <Sparkles className="mt-0.5 h-4 w-4 shrink-0 text-[var(--purple-ink)]" />
          <span className="min-w-0">
            <span className="block font-strong text-tx-0">{t('actions.new_anomaly')}</span>
            <span className="mt-0.5 block text-xs leading-relaxed text-tx-2">
              {t('actions.new_anomaly_description')}
            </span>
          </span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
