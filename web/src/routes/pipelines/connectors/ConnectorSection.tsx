import { Pencil, Plus, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { DataTable, type DataTableColumn } from '@/admin';
import type { Connector } from '@/api/connectors';
import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { DisabledControl } from '@/shell/DisabledControl';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { Switch } from '@/shell/ui/switch';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { KIND_ICONS } from './kindIcons';
import {
  AUTH_FIELDS,
  authModeOf,
  hasStoredSecret,
  isConnectorKind,
  keepsSecrets,
  placeOf,
  specOf,
  targetStreamOf,
  type Direction,
} from './kinds';
import { formatRelativeMicros } from '../presentation';
import { pipelineFlatTableClassName } from '../Surfaces';

/**
 * The connectors of one direction: the sources that bring logs in, or the
 * destinations pipelines deliver to. Each has its own columns, because what
 * matters about one is not what matters about the other.
 */
export function ConnectorSection({
  direction,
  connectors,
  access,
  busyId,
  onAdd,
  onEdit,
  onRemove,
  onToggle,
}: {
  direction: Direction;
  connectors: Connector[];
  access: ActionAccess;
  /** The connector being switched on or off. */
  busyId: string | null;
  onAdd: () => void;
  onEdit: (connector: Connector) => void;
  onRemove: (connector: Connector) => void;
  onToggle: (connector: Connector, enabled: boolean) => void;
}) {
  const { t, i18n } = useTranslation('pipelines');
  const headingId = `connectors-${direction}`;

  // The first two columns have one width in both sections, so the two tables read as one list.
  const name: DataTableColumn<Connector> = {
    key: 'name',
    header: t('connectors.columns.name'),
    width: 260,
    cell: (row) => <span className="font-strong text-tx-0">{row.name}</span>,
  };
  const kind: DataTableColumn<Connector> = {
    key: 'kind',
    header: t('connectors.columns.kind'),
    width: 260,
    cell: (row) => <KindCell kind={row.kind} />,
  };
  // The name is what a row is; the rest is said about it, in the lighter weight.
  const plain = 'font-body text-tx-1';
  const status: DataTableColumn<Connector> = {
    key: 'status',
    header: t('connectors.columns.status'),
    width: 84,
    cell: (row) => (
      <StatusSwitch
        connector={row}
        access={access}
        busy={busyId === row.id}
        onToggle={(enabled) => onToggle(row, enabled)}
      />
    ),
  };
  const actions: DataTableColumn<Connector> = {
    key: 'actions',
    header: <span className="sr-only">{t('connectors.columns.actions')}</span>,
    width: 84,
    cell: (row) => (
      <div className="flex items-center justify-end gap-[2px]">
        <RowAction label={t('connectors.edit')} access={access} onClick={() => onEdit(row)}>
          <Pencil aria-hidden className="h-3.5 w-3.5" />
        </RowAction>
        <RowAction label={t('connectors.remove.action')} access={access} onClick={() => onRemove(row)}>
          <Trash2 aria-hidden className="h-3.5 w-3.5" />
        </RowAction>
      </div>
    ),
  };
  const columns: DataTableColumn<Connector>[] =
    direction === 'source'
      ? [
          name,
          kind,
          {
            key: 'stream',
            header: t('connectors.columns.stream'),
            cell: (row) => <span className={cn('font-mono text-xs', plain)}>{targetStreamOf(row)}</span>,
          },
          {
            key: 'last_received',
            header: t('connectors.columns.last_received'),
            width: 150,
            cell: (row) => (
              <span className={cn(plain, !row.last_run_at_micros && 'text-tx-3')}>
                {row.last_run_at_micros
                  ? formatRelativeMicros(row.last_run_at_micros, i18n.language)
                  : t('connectors.never')}
              </span>
            ),
          },
          status,
          actions,
        ]
      : [
          name,
          kind,
          {
            key: 'place',
            header: t('connectors.columns.place'),
            cell: (row) => (
              <span className={cn('block max-w-[420px] truncate font-mono text-xs', plain)} title={placeOf(row)}>
                {placeOf(row)}
              </span>
            ),
          },
          {
            key: 'auth',
            header: t('connectors.columns.auth'),
            width: 190,
            cell: (row) => <span className={plain}>{t(`connectors.auth.modes.${authModeOf(row)}`)}</span>,
          },
          status,
          actions,
        ];

  return (
    <section
      data-connector-section={direction}
      aria-labelledby={headingId}
      className={cn('min-w-0 px-[16px] pb-[10px] pt-[12px]', surfacePanelClass)}
    >
      <header className="mb-[6px] flex min-h-[28px] items-center gap-[12px]">
        <h2 id={headingId} className="m-0 shrink-0 font-sans text-md font-strong text-tx-0">
          {t(`connectors.${direction}s.title`)}
        </h2>
        <span className="min-w-0 flex-1 truncate font-sans text-xs text-tx-3">
          {t(`connectors.${direction}s.description`)}
        </span>
        <ChromeButton
          size="sm"
          variant="ghost"
          className="shrink-0 text-tx-2"
          disabled={access.disabled}
          disabledReason={access.reason}
          onClick={onAdd}
        >
          <Plus aria-hidden className="h-3.5 w-3.5" />
          {t(`connectors.${direction}s.add`)}
        </ChromeButton>
      </header>
      {connectors.length === 0 ? (
        <p className="m-0 pb-[8px] pt-[6px] font-sans text-sm text-tx-3">{t(`connectors.${direction}s.empty`)}</p>
      ) : (
        <DataTable
          rows={connectors}
          rowKey={(row) => row.id}
          columns={columns}
          className={pipelineFlatTableClassName}
        />
      )}
    </section>
  );
}

function KindCell({ kind }: { kind: string }) {
  const { t } = useTranslation('pipelines');
  if (!isConnectorKind(kind)) return <span className="text-tx-3">{t('connectors.unknown_kind', { kind })}</span>;

  const Icon = KIND_ICONS[kind];
  const spec = specOf(kind);

  return (
    <span className="flex min-w-0 items-center gap-[8px] font-body">
      <Icon aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" strokeWidth={1.8} />
      <span className="truncate text-tx-1">{t(`connectors.kinds.${kind}`)}</span>
      {/* Sources differ in who starts the transfer; every destination is delivered to. */}
      {spec.direction === 'source' && (
        <span className="shrink-0 font-sans text-xs text-tx-3">{t(`connectors.transport.${spec.transport}`)}</span>
      )}
    </span>
  );
}

/**
 * Whether a connector can be switched without opening it. Switching sends its
 * whole configuration back; a backend that does not keep masked secrets would
 * save the masks, so there the switch is only for a connector with no secret.
 */
function switchable(connector: Connector): boolean {
  if (keepsSecrets(connector)) return true;
  if (!isConnectorKind(connector.kind)) return false;

  return !AUTH_FIELDS[authModeOf(connector)].some((field) => field.secret && hasStoredSecret(connector, field.key));
}

function StatusSwitch({
  connector,
  access,
  busy,
  onToggle,
}: {
  connector: Connector;
  access: ActionAccess;
  busy: boolean;
  onToggle: (enabled: boolean) => void;
}) {
  const { t } = useTranslation('pipelines');
  const safe = switchable(connector);
  const disabled = access.disabled || busy || !safe;

  return (
    <DisabledControl
      disabled={disabled && !busy}
      reason={access.disabled ? access.reason : !safe ? t('connectors.toggle_retype') : undefined}
    >
      <Switch
        checked={connector.enabled}
        disabled={disabled}
        aria-label={t('connectors.toggle', { name: connector.name })}
        onCheckedChange={onToggle}
      />
    </DisabledControl>
  );
}

function RowAction({
  label,
  access,
  onClick,
  children,
}: {
  label: string;
  access: ActionAccess;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="inline-flex">
          <IconButton aria-label={label} disabled={access.disabled} onClick={onClick}>
            {children}
          </IconButton>
        </span>
      </TooltipTrigger>
      <TooltipContent side="top">{access.disabled ? access.reason : label}</TooltipContent>
    </Tooltip>
  );
}
