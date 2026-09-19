// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use base64::Engine as _;
use rand::TryRng as _;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use super::{SyntheticService, model::validate_name};
use crate::{
    domain::synthetics::{ProbeAgentToken, ProbeAgentTokenInstructions, ProbeAgentTokenStatus},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Deserialize)]
pub struct CreateAgentTokenInput {
    pub name: String,
    pub location_id: Id,
    #[serde(default = "default_expiry_days")]
    pub expires_in_days: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RotateAgentTokenInput {
    #[serde(default = "default_expiry_days")]
    pub expires_in_days: u32,
}

const fn default_expiry_days() -> u32 {
    90
}

impl SyntheticService {
    pub async fn create_agent_token(
        &self,
        org_id: &Id,
        actor_id: &Id,
        input: CreateAgentTokenInput,
    ) -> Result<ProbeAgentTokenInstructions> {
        self.ensure_probe_registration_configured()?;
        validate_name(&input.name, 128, "Agent Token name")?;
        validate_expiry_days(input.expires_in_days)?;
        let now = TimestampMicros::now();
        let plaintext = generate_token()?;
        let token = self
            .repository
            .create_agent_token(
                ProbeAgentToken {
                    id: Id::new(),
                    organization_id: org_id.clone(),
                    location_id: input.location_id,
                    name: input.name.trim().to_string(),
                    token_prefix: token_prefix(&plaintext),
                    status: ProbeAgentTokenStatus::Active,
                    expires_at: expires_at(now, input.expires_in_days),
                    last_used_at: None,
                    created_by: actor_id.clone(),
                    created_at: now,
                    rotated_at: None,
                    disabled_at: None,
                    updated_at: now,
                },
                Sha256::digest(plaintext.as_bytes()).to_vec(),
            )
            .await?;
        Ok(self.agent_token_instructions(token, plaintext))
    }

    pub async fn list_agent_tokens(&self, org_id: &Id) -> Result<Vec<ProbeAgentToken>> {
        self.repository.list_agent_tokens(org_id).await
    }

    pub async fn rotate_agent_token(
        &self,
        org_id: &Id,
        token_id: &Id,
        input: RotateAgentTokenInput,
    ) -> Result<ProbeAgentTokenInstructions> {
        self.ensure_probe_registration_configured()?;
        validate_expiry_days(input.expires_in_days)?;
        let now = TimestampMicros::now();
        let plaintext = generate_token()?;
        let token = self
            .repository
            .rotate_agent_token(
                org_id,
                token_id,
                Sha256::digest(plaintext.as_bytes()).to_vec(),
                &token_prefix(&plaintext),
                expires_at(now, input.expires_in_days),
                now,
            )
            .await?;
        Ok(self.agent_token_instructions(token, plaintext))
    }

    pub async fn disable_agent_token(&self, org_id: &Id, token_id: &Id) -> Result<ProbeAgentToken> {
        self.repository
            .disable_agent_token(org_id, token_id, TimestampMicros::now())
            .await
    }

    fn ensure_probe_registration_configured(&self) -> Result<()> {
        if self.register_endpoint.is_empty()
            || self.control_endpoint.is_empty()
            || self.probe_ca_certificate_pem.is_empty()
        {
            return Err(Error::unavailable("Probe registration is not configured"));
        }
        Ok(())
    }

    fn agent_token_instructions(
        &self,
        token: ProbeAgentToken,
        plaintext: String,
    ) -> ProbeAgentTokenInstructions {
        let ca_base64 = base64::engine::general_purpose::STANDARD
            .encode(self.probe_ca_certificate_pem.as_bytes());
        let ca_sha256 = hex::encode(Sha256::digest(self.probe_ca_certificate_pem.as_bytes()));
        let command = format!(
            "docker run --rm -v molesignal-probe:/var/lib/molesignal-probe \
             ghcr.io/molesignal/probe-agent:latest register \
             --endpoint {} --token {} --ca-certificate-base64 {}",
            self.register_endpoint, plaintext, ca_base64
        );
        ProbeAgentTokenInstructions {
            token,
            agent_token: plaintext,
            register_endpoint: self.register_endpoint.clone(),
            control_endpoint: self.control_endpoint.clone(),
            ca_certificate_pem: self.probe_ca_certificate_pem.clone(),
            ca_sha256,
            command,
        }
    }
}

fn validate_expiry_days(days: u32) -> Result<()> {
    if days > 3_650 {
        return Err(Error::invalid(
            "Agent Token expiry must be between 1 and 3650 days, or 0 for no expiry",
        ));
    }
    Ok(())
}

fn expires_at(now: TimestampMicros, days: u32) -> Option<TimestampMicros> {
    (days > 0).then(|| {
        TimestampMicros(
            now.0
                .saturating_add(i64::from(days) * 24 * 60 * 60 * 1_000_000),
        )
    })
}

fn generate_token() -> Result<String> {
    let mut bytes = [0_u8; 32];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|error| Error::internal(format!("generate Agent Token: {error}")))?;
    Ok(format!(
        "ms_probe_{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    ))
}

fn token_prefix(plaintext: &str) -> String {
    plaintext.chars().take(17).collect()
}

#[cfg(test)]
mod tests {
    use super::{expires_at, generate_token, token_prefix, validate_expiry_days};
    use crate::shared::time::TimestampMicros;

    #[test]
    fn agent_tokens_are_random_and_have_a_safe_prefix() {
        let first = generate_token().expect("first token");
        let second = generate_token().expect("second token");

        assert!(first.starts_with("ms_probe_"));
        assert_eq!(first.len(), 52);
        assert_ne!(first, second);
        assert_eq!(token_prefix(&first), first[..17]);
    }

    #[test]
    fn expiry_supports_bounded_and_non_expiring_tokens() {
        let now = TimestampMicros(10);

        assert_eq!(expires_at(now, 0), None);
        assert_eq!(expires_at(now, 1), Some(TimestampMicros(86_400_000_010)));
        assert!(validate_expiry_days(3_650).is_ok());
        assert!(validate_expiry_days(3_651).is_err());
    }
}
