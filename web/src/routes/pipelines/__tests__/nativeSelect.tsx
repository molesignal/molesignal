import * as React from 'react';

/**
 * The shared select as a native one, for tests whose subject is what is
 * offered and picked, not the popover: Radix's select cannot be opened in
 * jsdom. Use with `vi.mock('@/shell/ui/select', () => import('./nativeSelect'))`.
 */
interface ItemProps {
  value: string;
  disabled?: boolean;
  children?: React.ReactNode;
}

function isElement(node: React.ReactNode): node is React.ReactElement<Record<string, unknown>> {
  return React.isValidElement(node);
}

/** Everything of one component type anywhere under `children`. */
function collect(
  children: React.ReactNode,
  type: unknown,
): Array<React.ReactElement<Record<string, unknown>>> {
  return React.Children.toArray(children).flatMap((child) => {
    if (!isElement(child)) return [];
    if (child.type === type) return [child];
    return collect(child.props.children as React.ReactNode, type);
  });
}

export function SelectItem(_props: ItemProps) {
  return null;
}

export function SelectTrigger(_props: { children?: React.ReactNode; 'aria-label'?: string }) {
  return null;
}

export function Select({
  value,
  onValueChange,
  disabled,
  children,
}: {
  value?: string;
  onValueChange?: (value: string) => void;
  disabled?: boolean;
  children?: React.ReactNode;
}) {
  const trigger = collect(children, SelectTrigger)[0];
  const items = collect(children, SelectItem);
  const known = items.some((item) => item.props.value === value);

  return (
    <select
      aria-label={trigger?.props['aria-label'] as string | undefined}
      value={known ? value : ''}
      disabled={disabled}
      onChange={(event) => onValueChange?.(event.target.value)}
    >
      {!known && <option value="" />}
      {items.map((item) => (
        <option
          key={item.props.value as string}
          value={item.props.value as string}
          disabled={Boolean(item.props.disabled)}
        >
          {item.props.children as React.ReactNode}
        </option>
      ))}
    </select>
  );
}

const Passthrough = ({ children }: { children?: React.ReactNode }) => <>{children}</>;

export const SelectContent = Passthrough;
export const SelectGroup = Passthrough;
export const SelectValue = () => null;
export const SelectLabel = () => null;
export const SelectSeparator = () => null;
