import { useQueryClient } from '@tanstack/react-query';
import { UserCheck, UserX } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, TableBulkBar, useBulkRun } from '@/admin';
import * as usersApi from '@/api/users';
import { ChromeButton } from '@/shell/chrome';

/** What can be done to the users ticked in the list. Disabling asks first: they lose their sign-in at once. */
export function UserBulkBar({ users, onClear }: { users: usersApi.UserView[]; onClear: () => void }) {
  const { t } = useTranslation('iam');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const [confirmingDisable, setConfirmingDisable] = React.useState(false);

  const run = useBulkRun({
    rows: users,
    onAllDone: onClear,
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['iam', 'users'] }),
  });

  return (
    <>
      <TableBulkBar count={users.length} onClear={onClear} data-testid="user-bulk">
        <ChromeButton
          size="sm"
          disabled={run.isPending}
          onClick={() => run.mutate((user) => usersApi.update(user.id, { disabled: false }))}
        >
          <UserCheck className="h-3 w-3" />
          {tc('actions.enable')}
        </ChromeButton>
        <ChromeButton size="sm" disabled={run.isPending} onClick={() => setConfirmingDisable(true)}>
          <UserX className="h-3 w-3" />
          {tc('actions.disable')}
        </ChromeButton>
      </TableBulkBar>
      <ConfirmDialog
        open={confirmingDisable}
        onOpenChange={setConfirmingDisable}
        destructive
        title={t('users.bulk_disable_title', { count: users.length })}
        description={t('users.bulk_disable_description')}
        confirmLabel={tc('actions.disable')}
        busy={run.isPending}
        onConfirm={() =>
          run.mutate((user) => usersApi.update(user.id, { disabled: true }), {
            onSettled: () => setConfirmingDisable(false),
          })
        }
      />
    </>
  );
}
