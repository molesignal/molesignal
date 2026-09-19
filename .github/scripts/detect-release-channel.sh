#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) 2026 MoleSignal Authors
#
# Detect which release channel a tag belongs to and emit downstream
# values (release title, docker float tag, docker versioned tag,
# prerelease flag). Used by .github/workflows/release.yml.
#
# Channel is inferred from branch reachability of the tag's commit:
#   stable (main) > rc > beta > alpha   (most stable wins)
# A commit promoted along alpha → beta → rc → main rides whichever branch
# "caught up"; the most stable branch becomes the release channel.
#
# Inputs (env):
#   GITHUB_REF_NAME   tag name, e.g. v1.2.3   (required)
#   GITHUB_SHA        commit SHA the tag points to   (required)
#
# Outputs:
#   stdout            key=value lines for local debugging
#   $GITHUB_OUTPUT    same lines (when running inside a GH Actions step)
#
# Exit codes:
#   0   channel detected
#   1   tag not reachable from main/rc/beta/alpha — release rejected
#   2   missing required env var

set -euo pipefail

: "${GITHUB_REF_NAME:?GITHUB_REF_NAME (tag name) must be set}"
: "${GITHUB_SHA:?GITHUB_SHA must be set}"

TAG="$GITHUB_REF_NAME"
SHA="$GITHUB_SHA"
VERSION="${TAG#v}"
VERSIONED_TAG="$VERSION"

# Branches must already be fetched as remote-tracking refs by the caller
# (the workflow does this in a preceding step).
CONTAINS=$(git branch -r --contains "$SHA" | sed -E 's/^[[:space:]]*//' || true)

on_branch() {
    printf '%s\n' "$CONTAINS" | grep -qFx "origin/$1"
}

if   on_branch main;  then
    CHANNEL=stable; PRERELEASE=false; FLOAT_TAG=latest; TITLE="$TAG"
elif on_branch rc;    then
    CHANNEL=rc;     PRERELEASE=true;  FLOAT_TAG=rc;     TITLE="$TAG (rc)"
elif on_branch beta;  then
    CHANNEL=beta;   PRERELEASE=true;  FLOAT_TAG=beta;   TITLE="$TAG (beta)"
elif on_branch alpha; then
    CHANNEL=alpha;  PRERELEASE=true;  FLOAT_TAG=alpha;  TITLE="$TAG (alpha)"
else
    echo "::error::tag ${TAG} (sha ${SHA}) is not reachable from origin/main, origin/rc, origin/beta, or origin/alpha; release only allowed from these branches" >&2
    echo "branches containing this sha:" >&2
    printf '%s\n' "$CONTAINS" >&2
    exit 1
fi

emit() {
    echo "channel=$CHANNEL"
    echo "version=$VERSION"
    echo "prerelease=$PRERELEASE"
    echo "float_tag=$FLOAT_TAG"
    echo "versioned_tag=$VERSIONED_TAG"
    echo "title=$TITLE"
}

# Human-readable echo (visible in CI logs and in local runs).
echo "Resolved release channel:"
emit | sed 's/^/  /'

# Machine-readable export for GitHub Actions step outputs.
if [ -n "${GITHUB_OUTPUT:-}" ]; then
    emit >> "$GITHUB_OUTPUT"
fi
