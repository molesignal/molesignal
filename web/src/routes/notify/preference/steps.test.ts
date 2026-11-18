import { describe, expect, it } from 'vitest';

import type { UserNotifyPreference } from '@/api/notify';

import { addedStep, freeBoosts, movedStep, savedSteps, stepKey, stepsOf, type Step } from './steps';

const FEISHU_BOOSTS = ['app', 'sms', 'phone'];

function preference(steps: Array<Partial<UserNotifyPreference['steps'][number]>>): UserNotifyPreference {
  return {
    id: 'p',
    organizationId: 'org',
    userId: 'u',
    category: 'alert',
    enabled: true,
    allowHighUrgencyBypass: true,
    createdAt: 1,
    updatedAt: 1,
    steps: steps.map((step, index) => ({
      id: `s${index}`,
      preferenceId: 'p',
      endpointId: 'e-lark',
      stepOrder: index + 1,
      delaySecs: 0,
      createdAt: 1,
      ...step,
    })),
  };
}

describe('a person’s notification steps', () => {
  it('reads the steps of one urgency in order, with their waits and boosts', () => {
    const saved = preference([
      { endpointId: 'e-lark', stepOrder: 2, delaySecs: 300, boost: 'phone' },
      { endpointId: 'e-lark', stepOrder: 1 },
      { endpointId: 'e-voice', stepOrder: 1, urgency: 'low' },
    ]);

    expect(stepsOf(saved, 'high')).toEqual([
      { endpointId: 'e-lark', wait: '0', boost: null },
      { endpointId: 'e-lark', wait: '5', boost: 'phone' },
    ]);
    expect(stepsOf(saved, 'low')).toEqual([{ endpointId: 'e-voice', wait: '0', boost: null }]);
    expect(stepsOf(undefined, 'high')).toEqual([]);
  });

  it('saves the methods, waits and boosts side by side; the first step waits for nothing', () => {
    const steps: Step[] = [
      { endpointId: 'e-lark', wait: '9', boost: null },
      { endpointId: 'e-lark', wait: '5', boost: 'phone' },
      { endpointId: 'e-voice', wait: '5000', boost: null },
    ];

    expect(savedSteps(steps)).toEqual({
      endpointIds: ['e-lark', 'e-lark', 'e-voice'],
      delaysSecs: [0, 300, 86_400],
      boosts: [null, 'phone', null],
    });
  });

  it('adds a method that pages harder again with a boost its other steps do not have', () => {
    const first = addedStep([], 'e-lark', FEISHU_BOOSTS, true);
    expect(first).toEqual({ endpointId: 'e-lark', wait: '0', boost: null });

    const second = addedStep([first!], 'e-lark', FEISHU_BOOSTS, true);
    expect(second?.boost).toBe('app');

    // Every boost taken: the method cannot come again.
    const all = [null, ...FEISHU_BOOSTS].map((boost) => ({ endpointId: 'e-lark', wait: '0', boost }));
    expect(addedStep(all, 'e-lark', FEISHU_BOOSTS, true)).toBeNull();

    // A method that cannot page harder comes once, and so does any method where steps do not wait.
    expect(addedStep([{ endpointId: 'e-voice', wait: '0', boost: null }], 'e-voice', [], true)).toBeNull();
    expect(addedStep([first!], 'e-lark', FEISHU_BOOSTS, false)).toBeNull();
  });

  it('keeps each boost to one step of a method, a step keeping its own', () => {
    const steps: Step[] = [
      { endpointId: 'e-lark', wait: '0', boost: null },
      { endpointId: 'e-lark', wait: '5', boost: 'phone' },
      { endpointId: 'e-dingtalk', wait: '5', boost: 'app' },
    ];

    expect(freeBoosts(steps, 'e-lark', FEISHU_BOOSTS, 1)).toEqual(['app', 'sms', 'phone']);
    expect(freeBoosts(steps, 'e-lark', FEISHU_BOOSTS, 0)).toEqual([null, 'app', 'sms']);
    expect(new Set(steps.map(stepKey)).size).toBe(3);
  });

  it('moves a step a place, and leaves the ends where they are', () => {
    const steps: Step[] = [
      { endpointId: 'a', wait: '0', boost: null },
      { endpointId: 'b', wait: '5', boost: null },
    ];

    expect(movedStep(steps, 1, -1).map((step) => step.endpointId)).toEqual(['b', 'a']);
    expect(movedStep(steps, 0, -1)).toBe(steps);
    expect(movedStep(steps, 1, 1)).toBe(steps);
  });
});
