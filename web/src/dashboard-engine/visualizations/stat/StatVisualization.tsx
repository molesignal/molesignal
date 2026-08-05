import { BigValue } from './BigValue';
import { prepareStatValues } from './model';
import { EmptyVisualization } from '../shared/EmptyVisualization';
import { calculationOption } from '../shared/reduction';
import type { VisualizationProps } from '../shared/types';

export function StatVisualization({
  data,
  options,
  height,
}: VisualizationProps) {
  const values = prepareStatValues(
    data.frames,
    calculationOption(options.calculation),
  );
  if (values.length === 0) return <EmptyVisualization />;

  const textMode = optionEnum(
    options.textMode,
    ['value', 'value_and_name', 'name'] as const,
    'value_and_name',
  );
  const graphMode = optionEnum(
    options.graphMode,
    ['none', 'area'] as const,
    'none',
  );
  const colorMode = optionEnum(
    options.colorMode,
    ['none', 'value', 'background'] as const,
    'value',
  );

  return (
    <div
      className="grid h-full min-h-0 overflow-auto"
      style={{
        gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 10rem), 1fr))',
        gridAutoRows: `minmax(${Math.min(112, Math.max(80, height / Math.ceil(values.length / 2)))}px, 1fr)`,
      }}
    >
      {values.map((item) => (
        <BigValue
          key={item.key}
          item={item}
          height={Math.max(80, height / Math.max(1, values.length))}
          textMode={textMode}
          graphMode={graphMode}
          colorMode={colorMode}
          showPercentChange={options.showPercentChange === true}
        />
      ))}
    </div>
  );
}

function optionEnum<const T extends readonly string[]>(
  value: unknown,
  choices: T,
  fallback: T[number],
): T[number] {
  return typeof value === 'string' && choices.includes(value)
    ? (value as T[number])
    : fallback;
}
