#!/usr/bin/env node

import { seedApm } from './apm.mjs';
import { API_BASE, Api, RUN_ID } from './shared.mjs';

async function main() {
  const api = new Api(API_BASE);
  await api.login();
  const apm = await seedApm(api);
  console.log(JSON.stringify({ run_id: RUN_ID, org_id: api.orgId, apm }, null, 2));
}

main().catch((error) => {
  console.error(error?.message ?? String(error));
  process.exitCode = 1;
});
