import { Upload } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as profilesApi from '@/api/profiles';
import { toApiError } from '@/lib/http';
import { Button } from '@/shell/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { Input } from '@/shell/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import { toast } from '@/shell/ui/sonner';

import { PROFILE_TYPE_KEYS, useProfileTypeLabel } from './shared';

/**
 * Direct pprof upload dialog. Streams the chosen file to
 * `POST /api/v1/profiles/upload` (gzip or raw protobuf — the backend sniffs);
 * `service` / `type` ride as query params. Setup for OTLP / Pyroscope lives in
 * Datasource — this is the quick "I have a .pprof" path.
 */
export function UploadProfileDialog({
  open,
  onOpenChange,
  defaultService = '',
  onUploaded,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  defaultService?: string | undefined;
  onUploaded?: (() => void) | undefined;
}) {
  const { t } = useTranslation('profiles');
  const { t: tc } = useTranslation('common');
  const typeLabel = useProfileTypeLabel();
  const [service, setService] = React.useState(defaultService);
  const [type, setType] = React.useState('cpu');
  const [file, setFile] = React.useState<File | null>(null);
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (open) {
      setService(defaultService);
      setType('cpu');
      setFile(null);
    }
  }, [open, defaultService]);

  const submit = async () => {
    if (!file) return;
    setBusy(true);
    try {
      await profilesApi.upload(file, {
        service: service.trim() || undefined,
        type: type || undefined,
      });
      toast.success(t('upload.success'));
      onUploaded?.();
      onOpenChange(false);
    } catch (e) {
      toast.error(`${t('upload.error')}: ${toApiError(e).message}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t('upload.title')}</DialogTitle>
          <DialogDescription>{t('upload.description')}</DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <label className="block space-y-1">
            <span className="font-sans text-xs text-tx-2">{t('upload.service')}</span>
            <Input
              value={service}
              onChange={(e) => setService(e.target.value)}
              placeholder={t('upload.service_placeholder')}
              className="h-8 font-sans text-xs"
            />
          </label>
          <label className="block space-y-1">
            <span className="font-sans text-xs text-tx-2">{t('upload.type')}</span>
            <Select value={type} onValueChange={setType}>
              <SelectTrigger className="h-8 font-sans text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PROFILE_TYPE_KEYS.map((k) => (
                  <SelectItem key={k} value={k} className="text-xs">
                    {typeLabel(k)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>
          <label className="block space-y-1">
            <span className="font-sans text-xs text-tx-2">{t('upload.file')}</span>
            <input
              type="file"
              accept=".pprof,.pb,.pb.gz,.gz,application/octet-stream"
              onChange={(e) => setFile(e.target.files?.[0] ?? null)}
              className="block w-full font-sans text-xs text-tx-1 file:mr-3 file:rounded file:border file:border-bd-1 file:bg-bg-2 file:px-2 file:py-1 file:font-sans file:text-xs file:text-tx-1 hover:file:bg-bg-3"
            />
          </label>
        </div>
        <DialogFooter>
          <Button variant="outline" size="sm" onClick={() => onOpenChange(false)} disabled={busy}>
            {tc('actions.cancel')}
          </Button>
          <Button size="sm" onClick={() => void submit()} disabled={!file || busy}>
            <Upload className="h-3.5 w-3.5" /> {busy ? t('upload.uploading') : t('upload.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
