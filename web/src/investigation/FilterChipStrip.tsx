import { X } from 'lucide-react';

import type { Filter } from '@/api/web';
import { Badge } from '@/shell/ui/badge';

interface FilterChipStripProps {
  filters: Filter[];
  inherited?: Set<string>;
  onRemove: (field: string) => void;
}

export function FilterChipStrip({ filters, inherited, onRemove }: FilterChipStripProps) {
  if (filters.length === 0) return null;
  return (
    <div className="flex flex-wrap gap-1 border-b border-border px-3 py-2">
      {filters.map((f) => {
        const isInherited = inherited?.has(f.field) ?? false;
        const value = Array.isArray(f.value) ? f.value.join(', ') : f.value;
        return (
          <Badge
            key={f.field}
            variant={isInherited ? 'secondary' : 'accent'}
            className="gap-1 pr-1"
          >
            <span className="font-sans text-xs">
              {f.field}
              <span className="text-muted-foreground">{` ${f.op} `}</span>
              {value}
            </span>
            <button
              type="button"
              onClick={() => onRemove(f.field)}
              className="rounded p-0.5 hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              aria-label={`Remove filter ${f.field}`}
            >
              <X className="h-3 w-3" />
            </button>
          </Badge>
        );
      })}
    </div>
  );
}
