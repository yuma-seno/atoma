#!/usr/bin/env bash
#
# The release pipeline, one step per `deploy` entry in
# .github/atomaton/config.yaml. Run one by hand with the same argument:
#
#   bash .github/atomaton/scripts/release.sh <step>
#
#   tag      on_merge  create v<Cargo.toml's version>, if that tag does not exist yet
#   prepare  on_tag    open the release as a draft, so a failed build publishes nothing
#   linux    on_tag    build the musl binary and attach it
#   macos    on_tag    build both darwin binaries and attach them
#   windows  on_tag    build the msvc binary and attach it
#   publish  on_tag    publish, once all four assets are attached
#
# `tag` runs on the merge that raises the version, and Atomaton starts the `on_tag`
# steps for the tag it creates (dispatch_new_tags). So no step here asks whether
# there is anything to release: the event already answered that. The alternative --
# every platform entry running on the merge and guarding itself -- would need each
# entry to skip itself on the merges that ship nothing, and a deploy list stops on
# failure only, with no way to say "nothing to do".
#
# Every step can run twice, so a half-finished release is recovered by re-running
# it from the Actions tab: `prepare` reuses the draft, the builds replace their
# assets with --clobber, and `publish` re-checks the assets before it publishes.
set -euo pipefail

repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is not set}"

# The asset name each platform step attaches. `publish` refuses a release that is
# missing one of them.
assets="atoma-linux-x86_64 atoma-macos-x86_64 atoma-macos-arm64 atoma-windows-x86_64.exe"

# The tag this deployment ships. Every step below but `tag` runs for a tag, so one
# started for a branch says so rather than publishing a draft named after it.
release() {
  case "${GITHUB_REF:-}" in
    refs/tags/*) printf '%s' "${GITHUB_REF_NAME}" ;;
    *)
      echo "::error::release.sh $1 ships a tag, but this run is for ${GITHUB_REF:-no ref}." >&2
      exit 1
      ;;
  esac
}

# One field of the release for a tag: $1 the tag, $2 the jq path. The list endpoint
# rather than the one by tag, because the list is the one that returns a draft to a
# token with push access -- which is what makes a re-run able to find it.
release_field() {
  gh api "repos/${repo}/releases?per_page=100" --jq ".[] | select(.tag_name == \"${1}\") | ${2}"
}

case "${1:-}" in
  tag)
    tag="v$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')"
    git fetch --tags --force --quiet
    if git rev-parse --quiet --verify "refs/tags/${tag}" >/dev/null; then
      echo "${tag} already exists; nothing to release."
      exit 0
    fi
    git config user.name "github-actions[bot]"
    git config user.email "github-actions[bot]@users.noreply.github.com"
    git tag "${tag}"
    git push origin "refs/tags/${tag}"
    echo "Created ${tag}; Atomaton starts its release once this run succeeds."
    ;;

  prepare)
    tag="$(release prepare)"
    if [ -n "$(release_field "${tag}" .id)" ]; then
      echo "The release for ${tag} already exists; its assets are replaced below."
    else
      # Opened through the API rather than `gh release create --draft` so that the
      # fields are exactly these: GitHub accepts `make_latest` only on a published
      # release, and this one is published -- and so becomes the latest -- at the
      # end, in `publish`.
      gh api --method POST "repos/${repo}/releases" \
        -f "tag_name=${tag}" \
        -f "name=atoma ${tag}" \
        -F draft=true \
        -F generate_release_notes=true >/dev/null
      echo "Opened a draft release for ${tag}."
    fi
    ;;

  linux)
    tag="$(release linux)"
    sudo apt-get install -y musl-tools
    rustup target add x86_64-unknown-linux-musl
    cargo build --release --locked --target x86_64-unknown-linux-musl
    cp target/x86_64-unknown-linux-musl/release/atoma atoma-linux-x86_64
    gh release upload "${tag}" atoma-linux-x86_64 --clobber
    ;;

  macos)
    tag="$(release macos)"
    rustup target add x86_64-apple-darwin aarch64-apple-darwin
    cargo build --release --locked --target x86_64-apple-darwin
    cargo build --release --locked --target aarch64-apple-darwin
    cp target/x86_64-apple-darwin/release/atoma atoma-macos-x86_64
    cp target/aarch64-apple-darwin/release/atoma atoma-macos-arm64
    gh release upload "${tag}" atoma-macos-x86_64 atoma-macos-arm64 --clobber
    ;;

  windows)
    tag="$(release windows)"
    rustup target add x86_64-pc-windows-msvc
    cargo build --release --locked --target x86_64-pc-windows-msvc
    cp target/x86_64-pc-windows-msvc/release/atoma.exe atoma-windows-x86_64.exe
    gh release upload "${tag}" atoma-windows-x86_64.exe --clobber
    ;;

  publish)
    tag="$(release publish)"
    attached="$(release_field "${tag}" '.assets[].name')"
    missing=""
    for asset in ${assets}; do
      printf '%s\n' "${attached}" | grep -qx "${asset}" || missing="${missing} ${asset}"
    done
    if [ -n "${missing}" ]; then
      echo "::error::${tag} is missing${missing}; it is left as a draft." >&2
      exit 1
    fi
    gh api --method PATCH "repos/${repo}/releases/$(release_field "${tag}" .id)" -F draft=false >/dev/null
    echo "Published ${tag}."
    ;;

  *)
    echo "usage: bash .github/atomaton/scripts/release.sh tag|prepare|linux|macos|windows|publish" >&2
    exit 2
    ;;
esac
