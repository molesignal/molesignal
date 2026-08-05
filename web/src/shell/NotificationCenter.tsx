import { useQuery } from '@tanstack/react-query';
import { Bell } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as usersApi from '@/api/users';
import { hasPermission, useProductAccess } from '@/product/access';
import { cn } from '@/shell/lib/cn';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';
import { useAuthStore } from '@/stores/auth';

/**
 * 顶栏通用消息通知中心。
 *
 * 通知来自多个「源」（source hook），在 {@link useAppNotifications} 聚合成统一的
 * {@link AppNotification} 列表后由本组件渲染。**新增一类通知 = 写一个 source hook
 * 并在 useAppNotifications 里合并即可**，渲染逻辑无需改动 - 审批只是当前的第一个源，
 * 而非唯一来源。
 */
export interface AppNotification {
  id: string;
  /** 通知类别（将来可据此分组 / 配图标）。 */
  kind: string;
  /** 已本地化的标题。 */
  title: string;
  /** 可选的次要描述。 */
  description?: string;
  /** 点击后跳转的路由。 */
  to?: string;
}

// ——————————————————————————————————————————————————————————
// 源 1：待审批的自助注册用户（仅管理员可见）。
// ——————————————————————————————————————————————————————————
function useApprovalNotifications(): AppNotification[] {
  const { t } = useTranslation('shell');
  const ctx = useAuthStore((s) => s.ctx);
  const access = useProductAccess();
  const canManageMembers = hasPermission('org.members.manage', access);

  const q = useQuery({
    queryKey: ['iam', 'users'],
    queryFn: () => usersApi.list(),
    enabled: canManageMembers && !!ctx,
    refetchInterval: 60_000,
  });

  return (q.data ?? [])
    .filter((u) => u.status === 'pending')
    .map((u) => ({
      id: `approval:${u.id}`,
      kind: 'approval',
      title: t('notifications.pending_user'),
      description: u.display_name || u.email,
      to: '/iam/approvals',
    }));
}

/** 聚合所有来源的通知。新增类型在此 push 一个新 source hook 的结果。 */
function useAppNotifications(): AppNotification[] {
  const approvals = useApprovalNotifications();
  // 将来：const alerts = useAlertNotifications(); return [...approvals, ...alerts];
  return [...approvals];
}

export function NotificationCenter() {
  const { t } = useTranslation('shell');
  const nav = useNavigate();
  const items = useAppNotifications();
  const count = items.length;

  return (
    <Popover>
      <PopoverTrigger
        title={t('topbar.notifications')}
        aria-label={t('topbar.notifications')}
        className={cn(
          'relative flex h-8 w-8 items-center justify-center rounded-md text-tx-2',
          'hover:bg-bg-3 hover:text-tx-0',
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
        )}
      >
        <Bell className="h-3.5 w-3.5" />
        {count > 0 && (
          <span className="absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-red" />
        )}
      </PopoverTrigger>
      <PopoverContent align="end" className="w-80 p-0">
        <div className="flex items-center justify-between border-b border-bd-0 px-3 py-2">
          <span className="font-sans text-xs font-strong text-tx-0">
            {t('notifications.title')}
          </span>
          {count > 0 && <span className="font-sans text-xs text-tx-3">{count}</span>}
        </div>
        {count === 0 ? (
          <div className="px-3 py-6 text-center font-sans text-xs text-tx-3">
            {t('notifications.empty')}
          </div>
        ) : (
          <ul className="max-h-80 overflow-auto py-1">
            {items.map((n) => (
              <li key={n.id}>
                <button
                  type="button"
                  onClick={() => n.to && nav(n.to)}
                  className="flex w-full flex-col items-start gap-0.5 px-3 py-2 text-left hover:bg-bg-3"
                >
                  <span className="font-sans text-xs text-tx-1">{n.title}</span>
                  {n.description && (
                    <span className="font-sans text-xs text-tx-3">{n.description}</span>
                  )}
                </button>
              </li>
            ))}
          </ul>
        )}
      </PopoverContent>
    </Popover>
  );
}
