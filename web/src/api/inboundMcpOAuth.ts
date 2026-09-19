import { http } from '@/lib/http';

export interface OAuthAuthorizationRequest {
  response_type: string;
  client_id: string;
  redirect_uri: string;
  scope: string;
  state?: string;
  code_challenge: string;
  code_challenge_method: string;
  resource: string;
}

export interface OAuthAuthorizationInspection {
  client: {
    client_id: string;
    client_name: string;
    client_uri?: string | null;
    metadata_document: boolean;
  };
  scope: string;
  resource: string;
  redirect_uri: string;
  state?: string | null;
  organization: {
    id: string;
    name: string;
  };
}

export async function inspectAuthorization(
  query: string,
): Promise<OAuthAuthorizationInspection> {
  const { data } = await http.get<OAuthAuthorizationInspection>(
    `/oauth/authorization-request?${query}`,
  );
  return data;
}

export async function decideAuthorization(
  request: OAuthAuthorizationRequest,
  approve: boolean,
): Promise<string> {
  const { data } = await http.post<{ redirect_to: string }>('/oauth/authorize', {
    ...request,
    approve,
  });
  return data.redirect_to;
}
