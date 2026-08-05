#!/usr/bin/env node

import { seedControlPlane } from './control-plane.mjs';
import { API_BASE, Api, RUN_ID } from './shared.mjs';

async function main() {
  const api = new Api(API_BASE);
  await api.login();
  const created = await seedControlPlane(api);
  console.log(JSON.stringify({ run_id: RUN_ID, org_id: api.orgId, created }, null, 2));
}

main().catch((error) => {
  console.error(error?.message ?? String(error));
  process.exitCode = 1;
});
