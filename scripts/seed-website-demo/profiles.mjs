import { NOW_MS } from './shared.mjs';

export async function seedProfiles(api) {
  const count = 18;
  for (let index = 0; index < count; index += 1) {
    const start = (NOW_MS - (index + 1) * 300_000) * 1_000;
    const query = new URLSearchParams({
      name: 'order-service.cpu{env=production,region=us-east-1,version=4.13.0}',
      format: 'folded',
      from: String(start),
      until: String(start + 60_000_000),
    });
    const body = [
      `runtime.main;http.server;orders.Handler.Create;orders.Workflow.Submit ${740 + index * 23}`,
      `runtime.main;http.server;orders.Handler.Create;orders.Repository.Save;sqlx.query ${480 + index * 17}`,
      `runtime.main;http.server;orders.Handler.Create;payments.Client.Authorize;tls.Conn.Read ${360 + index * 13}`,
      `runtime.main;http.server;orders.Handler.Create;inventory.Client.Reserve;json.Marshal ${255 + index * 9}`,
      `runtime.main;tokio.runtime;task.poll ${190 + index * 7}`,
    ].join('\n');
    const response = await fetch(`${api.base}/profiles/ingest?${query}`, {
      method: 'POST',
      headers: {
        authorization: `Bearer ${api.token}`,
        'content-type': 'text/plain; charset=utf-8',
      },
      body,
    });
    if (!response.ok) {
      throw new Error(
        `POST /profiles/ingest -> ${response.status} ${await response.text()}`,
      );
    }
  }
  return { profiles: count };
}
