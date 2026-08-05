import { RUN_ID } from './shared.mjs';

const SCHEDULES = [
  {
    name: 'Commerce Primary On-call · APAC',
    description: 'Primary APAC rotation for checkout and order processing.',
  },
  {
    name: 'Payments Secondary · Follow-the-sun',
    description: 'Secondary rotation for payment authorization and settlement.',
  },
  {
    name: 'Platform Incident Commander · Weekday',
    description: 'Weekday incident commander rotation for production services.',
  },
];

export async function seedOperations(api) {
  const schedules = [];
  for (const [index, schedule] of SCHEDULES.entries()) {
    const created = await api.post('/schedules', {
      ...schedule,
      timezone: 'Asia/Shanghai',
      enabled: true,
      rotations: [
        {
          id: `website-${RUN_ID}-${index + 1}`,
          name: index === 2 ? 'Incident commander' : 'Primary',
          members: [api.userId],
          kind: 'daily',
          start_at: Date.now() * 1_000 - 86_400_000_000,
        },
      ],
      overrides: [],
    });
    schedules.push(created.id);
  }
  return { schedules };
}
