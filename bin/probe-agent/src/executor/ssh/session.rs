// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow};
use russh::{
    ChannelMsg, client,
    keys::{PrivateKeyWithHashAlg, decode_secret_key},
};

use super::HostKeyVerifier;
use crate::{executor::ExecutionContext, protocol::v1::ssh_spec::Authentication};

const OUTPUT_MAX_BYTES: usize = 64 * 1024;

pub(super) async fn authenticate(
    session: &mut client::Handle<HostKeyVerifier>,
    authentication: &Authentication,
    context: &ExecutionContext,
) -> Result<(&'static str, bool)> {
    match authentication {
        Authentication::Password(value) => {
            let username = context.resolve(
                value
                    .username
                    .as_ref()
                    .ok_or_else(|| anyhow!("SSH username is required"))?,
            )?;
            let password = context.resolve(
                value
                    .password
                    .as_ref()
                    .ok_or_else(|| anyhow!("SSH password is required"))?,
            )?;
            Ok((
                "password",
                session
                    .authenticate_password(username, password)
                    .await?
                    .success(),
            ))
        }
        Authentication::PublicKey(value) => {
            let username = context.resolve(
                value
                    .username
                    .as_ref()
                    .ok_or_else(|| anyhow!("SSH username is required"))?,
            )?;
            let private_key = context.resolve(
                value
                    .private_key
                    .as_ref()
                    .ok_or_else(|| anyhow!("SSH private key is required"))?,
            )?;
            let passphrase = value
                .passphrase
                .as_ref()
                .map(|value| context.resolve(value))
                .transpose()?;
            let private_key = decode_secret_key(&private_key, passphrase.as_deref())
                .context("cannot decode SSH private key")?;
            let hash = session.best_supported_rsa_hash().await?.flatten();
            Ok((
                "public_key",
                session
                    .authenticate_publickey(
                        username,
                        PrivateKeyWithHashAlg::new(Arc::new(private_key), hash),
                    )
                    .await?
                    .success(),
            ))
        }
    }
}

pub(super) struct CommandResult {
    pub output: Vec<u8>,
    pub exit_status: Option<i32>,
    pub truncated: bool,
}

pub(super) async fn execute_command(
    session: &mut client::Handle<HostKeyVerifier>,
    command: &str,
) -> Result<CommandResult> {
    let mut channel = session.channel_open_session().await?;
    channel.exec(true, command.as_bytes()).await?;
    let mut output = Vec::new();
    let mut exit_status = None;
    let mut truncated = false;
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                let remaining = OUTPUT_MAX_BYTES.saturating_sub(output.len());
                output.extend_from_slice(&data[..data.len().min(remaining)]);
                truncated |= data.len() > remaining;
            }
            ChannelMsg::ExitStatus {
                exit_status: status,
            } => {
                exit_status = i32::try_from(status).ok();
            }
            _ => {}
        }
    }
    Ok(CommandResult {
        output,
        exit_status,
        truncated,
    })
}
