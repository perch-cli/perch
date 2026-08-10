#!/bin/sh
# Installs Perch.
#
#   curl -fsSL https://perch-cli.github.io/perch/install.sh | sh
#
# Environment:
#   PERCH_VERSION      a tag to install, such as v0.1.0. Default: the latest release.
#   PERCH_INSTALL_DIR  where to put the binary. Default: ~/.local/bin.
#
# POSIX sh, because the shell people pipe this into is not always bash — and
# `set -eu` from the first line, because the failure mode of an installer that
# keeps going after a failed download is a half-written binary on the PATH.

set -eu

REPO="perch-cli/perch"

# Overridable so the script can be tested against a fabricated release served
# locally. Nothing in normal use should set these.
API="${PERCH_API_BASE:-https://api.github.com/repos/$REPO}"
DOWNLOADS="${PERCH_DOWNLOAD_BASE:-https://github.com/$REPO/releases/download}"

INSTALL_DIR="${PERCH_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf 'perch: %s\n' "$1" >&2; }
die() {
    printf 'perch: %s\n' "$1" >&2
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || die "$1 is required and is not installed"
}

need curl
need tar

# ---------------------------------------------------------------- which build

os=$(uname -s)
arch=$(uname -m)

case "$os" in
Darwin)
    case "$arch" in
    arm64 | aarch64) target="aarch64-apple-darwin" ;;
    x86_64) target="x86_64-apple-darwin" ;;
    *) die "no Perch build for macOS on $arch" ;;
    esac
    ;;
Linux)
    # The Linux builds are musl, linked statically, so there is one per
    # architecture and it runs on glibc and musl systems alike.
    case "$arch" in
    aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
    x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
    *) die "no Perch build for Linux on $arch" ;;
    esac
    ;;
*)
    die "no Perch build for $os. On Windows, use install.ps1."
    ;;
esac

# -------------------------------------------------------------- which version

if [ -n "${PERCH_VERSION:-}" ]; then
    version="$PERCH_VERSION"
else
    # sed rather than jq: an installer should not need a JSON parser installed
    # before it can tell you what it would install.
    version=$(curl -fsSL "$API/releases/latest" |
        sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' |
        head -n1)
    [ -n "$version" ] || die "could not work out the latest release"
fi

archive="perch-${version}-${target}.tar.gz"
say "installing $version for $target"

# ------------------------------------------------------------------- download

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

curl -fsSL "$DOWNLOADS/$version/$archive" -o "$tmp/$archive" ||
    die "could not download $archive — is $version a release?"
curl -fsSL "$DOWNLOADS/$version/SHA256SUMS" -o "$tmp/SHA256SUMS" ||
    die "could not download SHA256SUMS for $version"

# --------------------------------------------------------------------- verify

if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$tmp/$archive" | cut -d' ' -f1)
elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$tmp/$archive" | cut -d' ' -f1)
else
    die "neither sha256sum nor shasum is installed, so the download cannot be verified"
fi

expected=$(awk -v f="$archive" '$2 == f { print $1 }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || die "SHA256SUMS for $version does not mention $archive"

if [ "$actual" != "$expected" ]; then
    die "checksum mismatch for $archive: expected $expected, got $actual"
fi
say "checksum ok"

# Perch is built by a public workflow and its provenance is signed, so anyone
# holding `gh` can confirm which workflow, in which repository, at which commit
# produced this exact file. That is a stronger claim than the checksum, which
# was fetched from the same place as the archive and so proves only that the
# two agree.
#
# Attempted only when `gh` is both installed and logged in, and then it is
# binding rather than advisory. The download above already succeeded, so the
# network is up and the token works — a verification that fails from here is
# saying something about the file, and an installer that shrugs at that is
# doing the check for decoration.
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    if gh attestation verify "$tmp/$archive" --repo "$REPO" >/dev/null 2>&1; then
        say "provenance ok — built by $REPO"
    else
        die "provenance check failed for $archive. It does not appear to have been built by $REPO. Not installing."
    fi
fi

# -------------------------------------------------------------------- install

tar -xzf "$tmp/$archive" -C "$tmp" perch
mkdir -p "$INSTALL_DIR"
# To a temporary name in the same directory and then moved, so a perch that is
# running right now is replaced rather than written through.
mv "$tmp/perch" "$INSTALL_DIR/.perch.$$"
chmod 755 "$INSTALL_DIR/.perch.$$"
mv "$INSTALL_DIR/.perch.$$" "$INSTALL_DIR/perch"

say "installed to $INSTALL_DIR/perch"

case ":$PATH:" in
*":$INSTALL_DIR:"*) ;;
*)
    say ""
    say "$INSTALL_DIR is not on your PATH. Add it:"
    say "    export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac

say "run 'perch status' to see where you are"
