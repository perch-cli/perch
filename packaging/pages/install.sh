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
# Asked before the mkdir, because only the mkdir knows the answer afterwards
# and the PATH advice below turns on it.
if [ -d "$INSTALL_DIR" ]; then dir_existed=yes; else dir_existed=no; fi
mkdir -p "$INSTALL_DIR"
# To a temporary name in the same directory and then moved, so a perch that is
# running right now is replaced rather than written through.
mv "$tmp/perch" "$INSTALL_DIR/.perch.$$"
chmod 755 "$INSTALL_DIR/.perch.$$"
mv "$INSTALL_DIR/.perch.$$" "$INSTALL_DIR/perch"

say "installed to $INSTALL_DIR/perch"

# ----------------------------------------------------------------------- path

# Nothing below writes to any file. An rc file is a document its owner edits by
# hand, and a line appended among their own lines could never be told back out
# again at removal time — so the installer advises here and writes only on
# Windows, where a PATH entry is a registry value that can be removed exactly
# (ADR 0033).

# The shells that read ~/.profile at login. zsh reads ~/.zprofile and fish
# reads neither, so the Debian guard can only be the answer for these.
reads_profile() {
    case "$1" in
    sh | bash | dash | ksh) return 0 ;;
    *) return 1 ;;
    esac
}

# Whether ~/.profile already carries a line naming the install directory.
#
# Debian and Ubuntu write their guard as `if [ -d "$HOME/.local/bin" ]`, with
# $HOME unexpanded — so a grep for the expanded path finds nothing on the very
# machines this is about. Both forms are looked for, and the tilde form too,
# for the people who wrote their own.
profile_names_install_dir() {
    # An install directory can be named outright, and then nothing else has
    # needed $HOME — so it is not certain to be set by the time we get here.
    [ -n "${HOME:-}" ] || return 1
    [ -r "$HOME/.profile" ] || return 1
    case "$INSTALL_DIR" in
    "$HOME"/*)
        rest=${INSTALL_DIR#"$HOME"/}
        # The tilde is a string being searched for, so it is meant not to expand.
        # shellcheck disable=SC2088
        grep -qF -e "$INSTALL_DIR" -e "\$HOME/$rest" -e "~/$rest" -- "$HOME/.profile"
        ;;
    *)
        grep -qF -e "$INSTALL_DIR" -- "$HOME/.profile"
        ;;
    esac
}

advise_about_path() {
    say ""

    # $SHELL, and not the parent process: under the documented
    # `curl … | sh` install the parent is that `sh`, so it reports the pipe's
    # shell and says nothing about the human being advised.
    shell_name=${SHELL:-}
    shell_name=${shell_name##*/}

    # We made the directory, and this machine's login file was always going to
    # add it — the guard was simply false at login, because there was nothing
    # there yet. A second export line is the wrong advice for that machine.
    if [ "$dir_existed" = no ] && reads_profile "$shell_name" && profile_names_install_dir; then
        say "$INSTALL_DIR is not on your PATH yet. Your ~/.profile adds it when it"
        say "exists, and it did not exist until a moment ago."
        say "Start a new login shell and it will be there."
        return
    fi

    # The tildes are how a person writes the path they are being pointed at, so
    # they are meant not to expand.
    # shellcheck disable=SC2088
    case "$shell_name" in
    bash) rc="~/.bashrc" ;;
    zsh) rc="~/.zshrc" ;;
    fish)
        # An export line is not valid fish, so printing one here would be a
        # visibly wrong answer rather than a merely unhelpful one.
        say "$INSTALL_DIR is not on your PATH. Add it to ~/.config/fish/config.fish:"
        say "    fish_add_path \"$INSTALL_DIR\""
        say "then restart your shell."
        return
        ;;
    *)
        # No file named, because naming one for a shell we did not recognise
        # would be a guess the reader has to go and check.
        say "$INSTALL_DIR is not on your PATH. Add it:"
        say "    export PATH=\"$INSTALL_DIR:\$PATH\""
        return
        ;;
    esac

    say "$INSTALL_DIR is not on your PATH. Add it to $rc:"
    say "    export PATH=\"$INSTALL_DIR:\$PATH\""
    say "then restart your shell."
}

case ":$PATH:" in
*":$INSTALL_DIR:"*) ;;
*) advise_about_path ;;
esac

say "run 'perch status' to see where you are"
