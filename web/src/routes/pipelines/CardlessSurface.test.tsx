import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Tabs, TabsList } from '@/shell/ui/tabs';

import {
  PipelineConfigSection,
  PipelineConfigValue,
  PipelineKpiBand,
  PipelineSection,
  PipelineTabTrigger,
} from './CardlessSurface';

describe('Pipelines surface hierarchy', () => {
  it('renders KPI and content as borderless surfaces with gutters', () => {
    const { container } = render(
      <>
        <PipelineKpiBand
          items={[
            { label: 'Success rate', value: '99.8%', tone: 'good' },
            { label: 'Processed rows', value: '24k' },
          ]}
        />
        <PipelineSection title="Topology" description="Source to sink">
          Graph
        </PipelineSection>
      </>,
    );

    const kpis = container.querySelector('[data-pipeline-kpis]');
    expect(kpis?.className).toContain('gap-[12px]');
    expect(kpis?.className).not.toMatch(/border/);
    expect(kpis?.firstElementChild?.className).toContain('rounded-md');
    expect(kpis?.firstElementChild?.className).toContain('shadow-functional-surface');
    expect(kpis?.firstElementChild?.className).not.toMatch(/\bborder/);

    const section = container.querySelector('[data-pipeline-section]');
    expect(section?.className).toContain('rounded-md');
    expect(section?.className).toContain('shadow-functional-surface');
    expect(section?.className).not.toMatch(/\bborder/);
    expect(section?.querySelector('header')?.className).not.toContain('border-b');
  });

  it('keeps configuration groups flat and tabs underline-driven', () => {
    const { container } = render(
      <>
        <Tabs defaultValue="overview">
          <TabsList>
            <PipelineTabTrigger value="overview">Overview</PipelineTabTrigger>
          </TabsList>
        </Tabs>
        <PipelineConfigSection title="Sources">
          <PipelineConfigValue>app_logs</PipelineConfigValue>
        </PipelineConfigSection>
      </>,
    );

    const trigger = container.querySelector('[role="tab"]');
    expect(trigger?.className).toContain('border-b-[3px]');
    expect(trigger?.className).toContain('data-[state=active]:border-indigo');
    expect(trigger?.className).toContain('shadow-none');

    const section = container.querySelector('[data-pipeline-config-section]');
    expect(section?.className).not.toMatch(/rounded|shadow|border/);

    const value = container.querySelector('[data-pipeline-config-value]');
    expect(value?.className).toContain('bg-bg-2');
    expect(value?.className).not.toMatch(/shadow|border/);
  });

  it('can remove the section header divider for visual workspaces', () => {
    const { container } = render(
      <PipelineSection title="Topology" showHeaderDivider={false}>
        Graph
      </PipelineSection>,
    );

    expect(container.querySelector('header')?.className).not.toContain('border-b');
  });
});
