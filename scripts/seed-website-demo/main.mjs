#!/usr/bin/env node
// Seed a coherent, production-like commerce workload for website screenshots.
// Data is synthetic and intentionally uses reserved example domains.

import { seedApm } from './apm.mjs';
import { seedProfiles } from './profiles.mjs';
import { seedRum } from './rum.mjs';
import {
  API_BASE,
  Api,
  RUN_ID,
  SERVICE_NAMES,
} from './shared.mjs';
import { seedSignals } from './signals.mjs';

async function main() {
  const api = new Api(API_BASE);
  await api.login();
  const apm = await seedApm(api);
  const signals = await seedSignals(api);
  const rum = await seedRum(api);
  const profiles = await seedProfiles(api);
  console.log(
    JSON.stringify(
      {
        run_id: RUN_ID,
        org_id: api.orgId,
        architecture: SERVICE_NAMES,
        apm,
        signals,
        rum,
        profiles,
      },
      null,
      2,
    ),
  );
}

main().catch((error) => {
  console.error(error?.message ?? String(error));
  process.exitCode = 1;
});
