#!/bin/sh
# Runs the real install.sh against a fabricated release and asserts on the
# advice it prints about PATH.
#
#   sh packaging/pages/install-test.sh
#
# The installer's download base is documented as overridable, and curl speaks
# file://, so a temporary directory holding a tarball and a SHA256SUMS is a
# whole release as far as the installer is concerned — no server, no network.
#
# Each case runs under `env -i` with a fabricated HOME and a PATH holding
# nothing but symlinks to the tools the installer needs. `gh` is deliberately
# not among them: the provenance check is binding when `gh` is installed and
# logged in, and a fabricated archive can never satisfy it. That is the check
# doing its job, and the way past it is to run without `gh` rather than to give
# the installer a flag that switches a security check off forever.

# Every tilde below is a fragment of text the installer prints for a person to
# read, and is meant not to expand.
# shellcheck disable=SC2088

set -eu

unset CDPATH
here=$(cd -- "$(dirname -- "$0")" && pwd)
installer="$here/install.sh"
version="v0.0.0-test"

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT INT TERM

# ------------------------------------------------------- a fabricated release

# The same mapping install.sh makes, because the archive has to be named what
# the installer will ask for.
case "$(uname -s)" in
Darwin)
    case "$(uname -m)" in
    arm64 | aarch64) target="aarch64-apple-darwin" ;;
    x86_64) target="x86_64-apple-darwin" ;;
    *) target="" ;;
    esac
    ;;
Linux)
    case "$(uname -m)" in
    aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
    x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
    *) target="" ;;
    esac
    ;;
*) target="" ;;
esac

# install.sh dies on a machine it has no build for, so a fallback guess here
# would fabricate an archive the installer is never going to ask for and report
# it as a failure of the advice.
if [ -z "$target" ]; then
    printf 'install-test: no Perch build for %s on %s, so there is nothing to test against\n' "$(uname -s)" "$(uname -m)" >&2
    exit 1
fi

archive="perch-${version}-${target}.tar.gz"
release="$root/downloads/$version"
mkdir -p "$release" "$root/build"

printf '#!/bin/sh\necho "not a real perch"\n' >"$root/build/perch"
chmod 755 "$root/build/perch"
(cd "$root/build" && tar -czf "$release/$archive" perch)

if command -v sha256sum >/dev/null 2>&1; then
    sum=$(sha256sum "$release/$archive" | cut -d' ' -f1)
else
    sum=$(shasum -a 256 "$release/$archive" | cut -d' ' -f1)
fi
printf '%s  %s\n' "$sum" "$archive" >"$release/SHA256SUMS"

# ------------------------------------------------------------- a bare PATH

shim="$root/shim"
mkdir -p "$shim"
for tool in awk chmod curl cut grep gzip head mkdir mktemp mv rm sed sha256sum shasum tar uname; do
    found=$(command -v "$tool" 2>/dev/null) || continue
    # Builtins report their own name rather than a path, and there is nothing
    # to link for those — the shell running the installer brings its own.
    case "$found" in
    /*) ln -s "$found" "$shim/$tool" ;;
    esac
done

if (PATH="$shim"; export PATH; command -v gh >/dev/null 2>&1); then
    printf 'install-test: gh is reachable from the shim PATH, so the provenance check would refuse the fabricated archive\n' >&2
    exit 1
fi

# --------------------------------------------------------------- the harness

cases=0
failures=0
name=""
output=""
home=""
install_dir=""

fail() {
    failures=$((failures + 1))
    printf 'not ok  %s\n        %s\n' "$name" "$1" >&2
    printf '%s\n' "$output" | sed 's/^/        | /' >&2
}

expect() {
    printf '%s\n' "$output" | grep -qF -- "$1" || fail "expected the output to mention '$1'"
}

refute() {
    if printf '%s\n' "$output" | grep -qF -- "$1"; then
        fail "expected the output not to mention '$1'"
    fi
}

# Runs the installer once, in a fabricated home of its own.
#
#   $1  what the case is called, for the report
#   $2  the value of $SHELL to run under, or "" to run with it unset
#   $3  what the fabricated home holds: none, or a ~/.profile carrying the
#       guard in its home, expanded or tilde form — or homeless, which is a run
#       with no $HOME set at all
#   $4  fresh, or precreated to create the install directory before the run
#   $5  where to install, relative to the fabricated home
run_case() {
    name=$1
    shell_value=$2
    fixture=$3
    precreate=$4
    relative_dir=$5
    cases=$((cases + 1))

    home="$root/home$cases"
    install_dir="$home/$relative_dir"
    mkdir -p "$home"

    # Debian and Ubuntu write this guard with $HOME unexpanded, which is the
    # form that a grep for the expanded path misses entirely. The single quotes
    # are what keeps the fixture in that form.
    # shellcheck disable=SC2016
    case "$fixture" in
    none | homeless) profile="" ;;
    home) profile='if [ -d "$HOME/.local/bin" ] ; then
    PATH="$HOME/.local/bin:$PATH"
fi' ;;
    expanded) profile="if [ -d \"$home/.local/bin\" ] ; then
    PATH=\"$home/.local/bin:\$PATH\"
fi" ;;
    tilde) profile='PATH=~/.local/bin:$PATH' ;;
    *)
        printf 'install-test: unknown home fixture %s\n' "$fixture" >&2
        exit 1
        ;;
    esac
    if [ -n "$profile" ]; then
        printf '%s\n' "$profile" >"$home/.profile"
    fi

    # Spelled out both ways rather than treating anything-but-precreated as
    # fresh: a typo there would quietly test the case that was already covered.
    case "$precreate" in
    precreated) mkdir -p "$install_dir" ;;
    fresh) ;;
    *)
        printf 'install-test: unknown directory state %s\n' "$precreate" >&2
        exit 1
        ;;
    esac

    # env(1) takes the assignments as arguments, so a variable the case wants
    # unset is simply one argument fewer.
    set -- \
        PATH="$shim" \
        PERCH_VERSION="$version" \
        PERCH_DOWNLOAD_BASE="file://$root/downloads" \
        PERCH_INSTALL_DIR="$install_dir"
    if [ "$fixture" != homeless ]; then
        set -- "$@" HOME="$home"
    fi
    if [ -n "$shell_value" ]; then
        set -- "$@" SHELL="$shell_value"
    fi

    output=$(env -i "$@" /bin/sh "$installer" 2>&1) ||
        fail "the installer exited non-zero"

    # Every case is also a check that the install itself worked, because advice
    # asserted against an installer that died early would pass by saying
    # nothing.
    expect "installed to $install_dir/perch"

    # The shim PATH deliberately has no `gh` on it, so every case here is also
    # the skipped-provenance case — and ADR 0039 says the one check that did
    # not happen is the one that most has to be said out loud. Silence here
    # reads exactly like a check that passed.
    expect "provenance not checked"

    # The Unix installer writes nothing but the install directory. Anything
    # else appearing under the fabricated home — a file or a directory — is the
    # whole decision undone, so this looks at both rather than at files alone.
    #
    # The directories on the way down to the install directory are the install
    # directory being created, which is the one thing the installer may do.
    stray=$(find "$home" -mindepth 1 ! -path "$install_dir" ! -path "$install_dir/*" ! -path "$home/.profile" |
        while IFS= read -r entry; do
            case "$install_dir/" in
            "$entry"/*) continue ;;
            esac
            printf '%s\n' "$entry"
        done)
    if [ -n "$stray" ]; then
        fail "the installer wrote outside the install directory: $stray"
    fi
    if [ -n "$profile" ] && [ "$(cat "$home/.profile")" != "$profile" ]; then
        fail "the installer edited ~/.profile"
    fi
}

report() {
    if [ "$failures" -eq 0 ]; then
        printf 'install-test: %s cases, all passed\n' "$cases"
    else
        printf 'install-test: %s cases, %s failed\n' "$cases" "$failures" >&2
        exit 1
    fi
}

# ------------------------------------------------------------------ the whole
# matrix

# Every combination of the three things the advice turns on: whether we made
# the directory, whether ~/.profile already names it, and which shell is being
# advised.
#
# One rule decides all sixteen — the new-login-shell answer belongs to a
# directory we created whose ~/.profile already names it, in a shell that reads
# that file, and every other cell gets the advice for its own shell — so each
# expectation is worked out from the rule rather than written out sixteen
# times. Writing them out is how a table ends up agreeing with the bug.
for created in fresh precreated; do
    for guard in none home; do
        for chosen_shell in /bin/bash /bin/zsh /usr/bin/fish ""; do
            case "$chosen_shell" in
            */bash) wanted="~/.bashrc" ;;
            */zsh) wanted="~/.zshrc" ;;
            */fish) wanted="fish_add_path" ;;
            *) wanted="export PATH=" ;;
            esac

            # bash reads ~/.profile at login. zsh reads ~/.zprofile, fish reads
            # neither, and an unset SHELL tells us nothing about the person.
            if [ "$created" = fresh ] && [ "$guard" = home ]; then
                case "$chosen_shell" in
                */bash) wanted="Start a new login shell" ;;
                esac
            fi

            run_case "$created directory, $guard guard, ${chosen_shell:-no \$SHELL}" \
                "$chosen_shell" "$guard" "$created" .local/bin
            expect "$wanted"
            if [ "$wanted" = "Start a new login shell" ]; then
                # The whole point of that answer is that it hands out no second
                # export line.
                refute "export PATH="
            else
                refute "Start a new login shell"
            fi
        done
    done
done

# --------------------------------------------------------- and the particulars

# An export line is not valid fish, so printing one would be a visibly wrong
# answer rather than a merely unhelpful one.
run_case "fish is told about fish_add_path and never about export" /usr/bin/fish none fresh .local/bin
expect "~/.config/fish/config.fish"
expect "fish_add_path"
refute "export"

# The syntax in full, once: the matrix asserts which file is named, and this
# asserts that what it says to put there is right.
run_case "the export line names the install directory" /bin/bash none fresh .local/bin
expect "Add it to ~/.bashrc:"
expect "export PATH=\"$install_dir:\$PATH\""

# tcsh is neither one of the three shells that get a file named nor one that
# reads ~/.profile, so it is the plain unrecognised case.
run_case "an unrecognised shell is named no file at all" /bin/tcsh home fresh .local/bin
expect "export PATH=\"$install_dir:\$PATH\""
refute "~/."

# ksh is not one of the three either, so it gets no file named — but it does
# read ~/.profile, so the guard is still the right answer for it.
run_case "a shell that reads ~/.profile without being one of the three" /bin/ksh home fresh .local/bin
expect "Start a new login shell"
refute "export PATH="

# The Debian guard is written with $HOME unexpanded, which is the form a grep
# for the expanded path misses. Both are looked for, and so is the tilde a
# hand-written one tends to use.
run_case "the expanded form of the guard counts too" /bin/bash expanded fresh .local/bin
expect "Start a new login shell"

run_case "and so does the tilde form" /bin/bash tilde fresh .local/bin
expect "Start a new login shell"

# A custom install directory needs no special handling: the guard names
# ~/.local/bin, the grep looks for what was installed into, and naming the
# shell's own file is the right answer for it.
run_case "a custom install directory is named its shell's file" /bin/bash home fresh opt/perch/bin
expect "~/.bashrc"
refute "Start a new login shell"

# An install directory named outright is the one way to get this far without
# anything having needed $HOME, and the script runs under `set -u`, where
# reading an unset one is fatal rather than empty.
run_case "no \$HOME at all is survivable" /bin/bash homeless fresh opt/perch/bin
expect "export PATH=\"$install_dir:\$PATH\""

report
