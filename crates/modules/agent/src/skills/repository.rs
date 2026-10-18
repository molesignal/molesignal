// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 从代码仓库拉取 Skill：仓库地址与分支 / 标签 → 压缩包地址与认证头。
//!
//! 不走 git 协议：按托管方的接口下载某个分支或标签的 `.tar.gz`。GitHub（含企业版）、
//! GitLab、Gitea / Forgejo、Bitbucket 由地址或所选的托管方式决定地址格式；其他托管方填
//! 含 `{ref}` 的压缩包地址。

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::shared::{Error, Result};

/// 仓库在哪种托管服务上。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoHost {
    Github,
    Gitlab,
    Gitea,
    Bitbucket,
    /// 直接给出压缩包地址。
    Archive,
}

impl RepoHost {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Gitlab => "gitlab",
            Self::Gitea => "gitea",
            Self::Bitbucket => "bitbucket",
            Self::Archive => "archive",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "github" => Ok(Self::Github),
            "gitlab" => Ok(Self::Gitlab),
            "gitea" => Ok(Self::Gitea),
            "bitbucket" => Ok(Self::Bitbucket),
            "archive" => Ok(Self::Archive),
            other => Err(Error::invalid(format!("unknown repository host `{other}`"))),
        }
    }

    /// 由公开托管站的域名认出；自建的要另选。
    pub fn detect(url: &Url) -> Option<Self> {
        match url.host_str()?.to_ascii_lowercase().as_str() {
            "github.com" | "www.github.com" => Some(Self::Github),
            "gitlab.com" | "www.gitlab.com" => Some(Self::Gitlab),
            "codeberg.org" | "gitea.com" => Some(Self::Gitea),
            "bitbucket.org" | "www.bitbucket.org" => Some(Self::Bitbucket),
            _ => None,
        }
    }
}

/// 一次下载：地址与认证方式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveRequest {
    pub url: String,
    /// 带令牌时用的请求头名与值的前缀（`Bearer `、`token ` 或空）。
    pub auth_header: &'static str,
    pub auth_prefix: &'static str,
}

impl ArchiveRequest {
    /// 带上令牌的请求头。
    pub fn auth(&self, token: &str) -> (&'static str, String) {
        (self.auth_header, format!("{}{token}", self.auth_prefix))
    }
}

/// 仓库地址校验：只接受 https，不许在地址里写凭据。
pub fn parse_repo_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw.trim())
        .map_err(|error| Error::invalid(format!("invalid repository address: {error}")))?;

    if url.scheme() != "https" {
        return Err(Error::invalid("the repository address must use https"));
    }

    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::invalid(
            "put the access token in its own field, not in the address",
        ));
    }

    if url.host_str().is_none() {
        return Err(Error::invalid("the repository address needs a host"));
    }

    Ok(url)
}

/// 仓库在托管站上的路径段：`owner/repo`，GitLab 可以有子组。
fn repo_path(url: &Url) -> Result<Vec<String>> {
    let mut segments: Vec<String> = url
        .path_segments()
        .map(|parts| {
            parts
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    // 浏览器里复制来的地址常带 `/-/tree/main`、`/tree/main`、`/src/branch/main`。
    if let Some(cut) = segments
        .iter()
        .position(|part| matches!(part.as_str(), "-" | "tree" | "blob" | "src"))
    {
        segments.truncate(cut);
    }

    if let Some(last) = segments.last_mut()
        && let Some(stripped) = last.strip_suffix(".git")
    {
        *last = stripped.to_string();
    }

    if segments.len() < 2 {
        return Err(Error::invalid(
            "the repository address should name an owner and a repository",
        ));
    }

    Ok(segments)
}

/// 除了不需要转义的 `-._~`，其余都转义：分支名里的 `/` 也转成 `%2F`。
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

fn encode(value: &str) -> String {
    utf8_percent_encode(value, SEGMENT).to_string()
}

/// 某个分支或标签的压缩包；`git_ref` 为空时取默认分支（Gitea 与 Bitbucket 要写明）。
pub fn archive_request(
    host: RepoHost,
    repo_url: &Url,
    git_ref: &str,
    archive_url: Option<&str>,
) -> Result<ArchiveRequest> {
    let git_ref = git_ref.trim();
    let origin = format!(
        "https://{}{}",
        repo_url.host_str().unwrap_or_default(),
        repo_url
            .port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default()
    );

    let needs_ref = || {
        if git_ref.is_empty() {
            Err(Error::invalid(
                "this host needs a branch or tag to download",
            ))
        } else {
            Ok(())
        }
    };

    let request = |url: String, auth_header, auth_prefix| ArchiveRequest {
        url,
        auth_header,
        auth_prefix,
    };

    match host {
        RepoHost::Github => {
            let path = repo_path(repo_url)?;
            let (owner, repo) = (&path[0], &path[1]);

            let api = if repo_url.host_str() == Some("github.com")
                || repo_url.host_str() == Some("www.github.com")
            {
                "https://api.github.com".to_string()
            } else {
                format!("{origin}/api/v3")
            };

            let tail = if git_ref.is_empty() {
                String::new()
            } else {
                format!("/{git_ref}")
            };

            Ok(request(
                format!("{api}/repos/{owner}/{repo}/tarball{tail}"),
                "authorization",
                "Bearer ",
            ))
        }
        RepoHost::Gitlab => {
            let project = encode(&repo_path(repo_url)?.join("/"));

            let query = if git_ref.is_empty() {
                String::new()
            } else {
                format!("?sha={}", encode(git_ref))
            };

            Ok(request(
                format!("{origin}/api/v4/projects/{project}/repository/archive.tar.gz{query}"),
                "private-token",
                "",
            ))
        }
        RepoHost::Gitea => {
            needs_ref()?;
            let path = repo_path(repo_url)?;

            Ok(request(
                format!(
                    "{origin}/api/v1/repos/{}/{}/archive/{}.tar.gz",
                    path[0],
                    path[1],
                    encode(git_ref)
                ),
                "authorization",
                "token ",
            ))
        }
        RepoHost::Bitbucket => {
            needs_ref()?;
            let path = repo_path(repo_url)?;

            Ok(request(
                format!(
                    "{origin}/{}/{}/get/{}.tar.gz",
                    path[0],
                    path[1],
                    encode(git_ref)
                ),
                "authorization",
                "Bearer ",
            ))
        }
        RepoHost::Archive => {
            let template = archive_url
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| Error::invalid("give the address of the archive"))?;

            let url = template.replace("{ref}", &encode(git_ref));
            parse_repo_url(&url)?;

            Ok(request(url, "authorization", "Bearer "))
        }
    }
}

/// 仓库里的一个目录作 Skill 来源时的路径：去掉首尾的 `/`，不许 `..`。
pub fn normalize_repo_dir(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_matches('/');

    if trimmed.split('/').any(|part| part == "..") {
        return Err(Error::invalid(
            "the folder in the repository must not use `..`",
        ));
    }

    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> Url {
        parse_repo_url(raw).unwrap()
    }

    #[test]
    fn github_downloads_through_its_api() {
        let repo = url("https://github.com/acme/sre-skills.git");

        assert_eq!(RepoHost::detect(&repo), Some(RepoHost::Github));
        assert_eq!(
            archive_request(RepoHost::Github, &repo, "main", None)
                .unwrap()
                .url,
            "https://api.github.com/repos/acme/sre-skills/tarball/main"
        );
        assert_eq!(
            archive_request(RepoHost::Github, &repo, "", None)
                .unwrap()
                .url,
            "https://api.github.com/repos/acme/sre-skills/tarball"
        );

        let enterprise = url("https://git.acme.dev/platform/skills/tree/main/ops");
        assert_eq!(
            archive_request(RepoHost::Github, &enterprise, "v1", None)
                .unwrap()
                .url,
            "https://git.acme.dev/api/v3/repos/platform/skills/tarball/v1"
        );
    }

    #[test]
    fn gitlab_keeps_subgroups_and_encodes_the_ref() {
        let repo = url("https://gitlab.acme.dev/sre/oncall/skills/-/tree/main");
        let request = archive_request(RepoHost::Gitlab, &repo, "release/1.0", None).unwrap();

        assert_eq!(
            request.url,
            "https://gitlab.acme.dev/api/v4/projects/sre%2Foncall%2Fskills/repository/archive.tar.gz?sha=release%2F1.0"
        );
        assert_eq!(request.auth("t"), ("private-token", "t".to_string()));
    }

    #[test]
    fn gitea_and_bitbucket_need_a_ref() {
        let repo = url("https://codeberg.org/acme/skills");

        assert!(archive_request(RepoHost::Gitea, &repo, "", None).is_err());
        assert_eq!(
            archive_request(RepoHost::Gitea, &repo, "main", None)
                .unwrap()
                .url,
            "https://codeberg.org/api/v1/repos/acme/skills/archive/main.tar.gz"
        );
        assert_eq!(
            archive_request(
                RepoHost::Bitbucket,
                &url("https://bitbucket.org/acme/skills"),
                "v2",
                None
            )
            .unwrap()
            .url,
            "https://bitbucket.org/acme/skills/get/v2.tar.gz"
        );
    }

    #[test]
    fn any_other_host_gives_its_archive_address() {
        let repo = url("https://git.example.net/skills");
        let request = archive_request(
            RepoHost::Archive,
            &repo,
            "main",
            Some("https://git.example.net/archive/{ref}.tar.gz"),
        )
        .unwrap();

        assert_eq!(request.url, "https://git.example.net/archive/main.tar.gz");
        assert!(archive_request(RepoHost::Archive, &repo, "main", None).is_err());
    }

    #[test]
    fn addresses_are_https_without_credentials() {
        assert!(parse_repo_url("http://github.com/a/b").is_err());
        assert!(parse_repo_url("https://user:pw@github.com/a/b").is_err());
        assert!(
            archive_request(RepoHost::Github, &url("https://github.com/acme"), "", None).is_err()
        );
    }

    #[test]
    fn a_folder_in_the_repository_stays_inside_it() {
        assert_eq!(normalize_repo_dir("/skills/ops/").unwrap(), "skills/ops");
        assert_eq!(normalize_repo_dir("").unwrap(), "");
        assert!(normalize_repo_dir("skills/../..").is_err());
    }
}
