// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Where a service's code lives, as people write it: an `http(s)` address, an
//! `ssh`, `git` or `git+ssh` URL, or git's own `git@host:org/repo.git`. No
//! credential is kept: a password becomes [`REPOSITORY_ADDRESS_MASK`], and so
//! does the whole user part of an `http(s)` address, where git carries tokens
//! in place of a user name. A login name such as the `git` of an SSH address
//! stays.

/// What a credential in a repository address is kept as.
pub const REPOSITORY_ADDRESS_MASK: &str = "****";

/// The longest repository address a definition keeps.
pub const REPOSITORY_ADDRESS_MAX_LENGTH: usize = 2_048;

const SCHEMES: [&str; 6] = ["https", "http", "ssh", "git", "git+ssh", "ssh+git"];

/// The address with its credentials masked; `None` when it is not one.
pub fn masked_repository_address(value: &str) -> Option<String> {
    let value = value.trim();

    if value.is_empty()
        || value.len() > REPOSITORY_ADDRESS_MAX_LENGTH
        || value
            .chars()
            .any(|char| char.is_whitespace() || char.is_control())
    {
        return None;
    }

    match value.split_once("://") {
        Some((scheme, rest)) => {
            let scheme = scheme.to_ascii_lowercase();
            SCHEMES.contains(&scheme.as_str()).then_some(())?;
            url_address(value.len() - rest.len(), value, &scheme)
        }
        None => scp_address(value),
    }
}

/// `scheme://[user[:password]@]host[:port][/path]`, the user part masked as
/// the module says.
fn url_address(authority_at: usize, value: &str, scheme: &str) -> Option<String> {
    let rest = &value[authority_at..];
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);

    let (user, host) = match authority.rsplit_once('@') {
        Some((user, host)) => (Some(user), host),
        None => (None, authority),
    };

    if !is_host(host.rsplit_once(':').map_or(host, |(name, port)| {
        if port.chars().all(|char| char.is_ascii_digit()) {
            name
        } else {
            host
        }
    })) {
        return None;
    }

    let user = user.map(|user| match user.split_once(':') {
        Some((name, _password)) => format!("{name}:{REPOSITORY_ADDRESS_MASK}"),
        None if scheme.starts_with("http") => REPOSITORY_ADDRESS_MASK.to_owned(),
        None => user.to_owned(),
    });

    Some(match user {
        Some(user) => format!("{}{user}@{host}{tail}", &value[..authority_at]),
        None => value.to_owned(),
    })
}

/// `user@host:path`, as git writes an SSH address without a scheme.
fn scp_address(value: &str) -> Option<String> {
    let (user, rest) = value.split_once('@')?;
    let (host, path) = rest.split_once(':')?;

    if user.is_empty() || !is_host(host) || path.is_empty() || path.starts_with("//") {
        return None;
    }

    // scp has no password, but one written in anyway is masked all the same.
    let user = match user.split_once(':') {
        Some((name, _password)) => format!("{name}:{REPOSITORY_ADDRESS_MASK}"),
        None => user.to_owned(),
    };

    let user_shaped = user
        .chars()
        .all(|char| char.is_ascii_alphanumeric() || "._-:*".contains(char));

    user_shaped.then(|| format!("{user}@{host}:{path}"))
}

/// A host name or an address: letters, digits, `.` and `-`, or an IPv6 address in brackets.
fn is_host(host: &str) -> bool {
    if let Some(inner) = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
    {
        return !inner.is_empty()
            && inner
                .chars()
                .all(|char| char.is_ascii_hexdigit() || ":.".contains(char));
    }

    !host.is_empty()
        && !host.starts_with(['.', '-'])
        && host
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || ".-".contains(char))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(value: &str) -> Option<String> {
        masked_repository_address(value)
    }

    #[test]
    fn web_and_git_addresses_are_kept_as_written() {
        for address in [
            "https://github.com/acme/checkout",
            "http://git.corp.example:8080/team/checkout.git",
            "git@github.com:acme/checkout.git",
            "ssh://git@git.corp.example:2222/team/checkout.git",
            "git://git.kernel.org/pub/scm/git/git.git",
            "git+ssh://git@github.com/acme/checkout.git",
        ] {
            assert_eq!(kept(address).as_deref(), Some(address), "{address}");
        }

        assert_eq!(
            kept("  https://github.com/acme/checkout ").as_deref(),
            Some("https://github.com/acme/checkout")
        );
    }

    #[test]
    fn a_password_or_an_http_token_is_masked() {
        assert_eq!(
            kept("https://deploy:s3cr3t@git.corp.example/team/checkout.git").as_deref(),
            Some("https://deploy:****@git.corp.example/team/checkout.git")
        );
        assert_eq!(
            kept("https://ghp_abc123@github.com/acme/checkout.git").as_deref(),
            Some("https://****@github.com/acme/checkout.git")
        );
        assert_eq!(
            kept("ssh://git:hunter2@git.corp.example/team/checkout.git").as_deref(),
            Some("ssh://git:****@git.corp.example/team/checkout.git")
        );
        assert_eq!(
            kept("deploy:hunter2@git.corp.example:team/checkout.git").as_deref(),
            Some("deploy:****@git.corp.example:team/checkout.git")
        );
        // A password holding `@` is masked whole: the host is after the last one.
        assert_eq!(
            kept("https://ci:p@ss@git.corp.example/team/checkout.git").as_deref(),
            Some("https://ci:****@git.corp.example/team/checkout.git")
        );
        // What is kept is masked already, so it is kept as it is.
        assert_eq!(
            kept("https://ci:****@git.corp.example/x.git").as_deref(),
            Some("https://ci:****@git.corp.example/x.git")
        );
    }

    #[test]
    fn anything_else_is_not_a_repository_address() {
        for value in [
            "",
            "github.com/acme/checkout",
            "ftp://example.com/repo.git",
            "javascript:alert(1)",
            "https://",
            "https://exa mple.com/repo",
            "git@:acme/checkout.git",
            "git@github.com:",
            "C:\\code\\checkout",
        ] {
            assert_eq!(kept(value), None, "{value}");
        }

        assert_eq!(
            kept(&format!("https://x.example/{}", "a".repeat(2_048))),
            None
        );
    }
}
