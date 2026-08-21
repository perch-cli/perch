#!/bin/sh
# Runs the real build.mjs against fabricated binaries and asserts on the six
# packages it assembles.
#
#   sh packaging/npm/build-test.sh
#
# build.mjs copies the binaries it is handed without looking inside them, so a
# directory of text files is a Release's worth of build output. What is asserted
# is the tree it wrote, because that is what a Release publishes and the command
# line is the only interface the script has.

set -eu

unset CDPATH
here=$(cd -- "$(dirname -- "$0")" && pwd)
build="$here/build.mjs"
# The one path build.mjs resolves outside its own directory: the README it puts
# in the wrapper is the repository's, reached by walking up out of here. Every
# other path it uses is either inside this directory or named on the command
# line.
readme="$here/../../README.md"
# A version no release will ever carry, so a failure names something obviously
# from this file rather than something that looks plausible.
version="0.0.0-test"

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT INT TERM

bins="$root/bins"
out="$root/out"

# The five targets, each with the package it becomes, the `os` and `cpu` pair
# npm resolves against, and the name its executable goes in under. Written out
# here rather than read out of build.mjs: a table taken from the thing under
# test agrees with it whatever it says.
targets="aarch64-apple-darwin @perch-cli/darwin-arm64 darwin arm64 perch
x86_64-apple-darwin @perch-cli/darwin-x64 darwin x64 perch
aarch64-unknown-linux-musl @perch-cli/linux-arm64 linux arm64 perch
x86_64-unknown-linux-musl @perch-cli/linux-x64 linux x64 perch
x86_64-pc-windows-msvc @perch-cli/win32-x64 win32 x64 perch.exe"

# ---------------------------------------------------- fabricated build output

# The Windows target is exercised by writing a file called perch.exe, which any
# machine can do — which is why this test needs no Windows leg.
while read -r target _ _ _ exe; do
    mkdir -p "$bins/$target"
    printf 'not a real perch for %s\n' "$target" >"$bins/$target/$exe"
done <<EOF
$targets
EOF

if ! node "$build" "$version" "$bins" "$out" >"$root/build.log" 2>&1; then
    printf 'build-test: build.mjs exited non-zero\n' >&2
    sed 's/^/        | /' "$root/build.log" >&2
    exit 1
fi

# --------------------------------------------------------------- the harness

cases=0
failures=0

fail() {
    failures=$((failures + 1))
    printf 'not ok  %s\n        %s\n' "$1" "$2" >&2
}

equals() {
    cases=$((cases + 1))
    if [ "$2" != "$3" ]; then
        fail "$1" "expected '$3', got '$2'"
    fi
}

executable() {
    cases=$((cases + 1))
    if [ ! -x "$2" ]; then
        fail "$1" "$2 is not executable"
    fi
}

same() {
    cases=$((cases + 1))
    if ! cmp -s "$2" "$3"; then
        fail "$1" "$2 is not a copy of $3"
    fi
}

# Reads one field out of a package.json, so what is asserted is the field rather
# than the formatting. A manifest that is not there reads as a missing field,
# because a package the build never wrote is a result this test asserts on; one
# that is there and is not JSON is left to throw.
field() {
    node -e '
        const { existsSync, readFileSync } = require("node:fs");
        const [file, path] = process.argv.slice(1);
        const value = existsSync(file)
            ? path
                  .split(".")
                  .reduce((held, key) => held?.[key], JSON.parse(readFileSync(file, "utf8")))
            : undefined;
        process.stdout.write(value === undefined ? "<missing>" : String(value));
    ' "$1" "$2"
}

count() {
    find "$1" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' '
}

report() {
    if [ "$failures" -eq 0 ]; then
        printf 'build-test: %s cases, all passed\n' "$cases"
    else
        printf 'build-test: %s cases, %s failed\n' "$cases" "$failures" >&2
        exit 1
    fi
}

# ----------------------------------------------------------- the five targets

wrapper="$out/perch-cli/package.json"

while read -r target pkg os cpu exe; do
    dir="$out/$(printf '%s' "$pkg" | tr / -)"
    manifest="$dir/package.json"

    # The whole of a platform package is one executable, under the name the
    # wrapper will go looking for on that platform.
    same "$pkg holds the binary it was given" "$dir/bin/$exe" "$bins/$target/$exe"
    # npm preserves the mode it finds, and what it finds after an unzip on a
    # Windows runner is not executable. A binary published without this bit is
    # an install that resolves and then cannot run.
    executable "$pkg's binary is executable" "$dir/bin/$exe"

    equals "$pkg is named for itself" "$(field "$manifest" name)" "$pkg"
    equals "$pkg carries the version" "$(field "$manifest" version)" "$version"
    # The pair npm resolves the optional dependencies against: one wrong and a
    # machine either installs a binary it cannot run or installs none at all.
    equals "$pkg is for $os" "$(field "$manifest" os)" "$os"
    equals "$pkg is for $cpu" "$(field "$manifest" cpu)" "$cpu"

    # By exact version, which is the reason the number is written by the script
    # rather than kept in the file. A range here is an install that resolves a
    # binary from some other release.
    equals "the wrapper depends on $pkg at $version" \
        "$(field "$wrapper" "optionalDependencies.$pkg")" "$version"
done <<EOF
$targets
EOF

# -------------------------------------------------------------- the wrapper

# One package per Target and the wrapper, and nothing beside them: a Release
# publishes what it finds here, and everything is counted rather than directories
# alone, because a stray file is published too.
equals "the build wrote six packages" "$(count "$out")" 6

equals "the wrapper carries the version" "$(field "$wrapper" version)" "$version"

same "the wrapper holds the launcher" "$out/perch-cli/bin/perch.js" "$here/perch-cli/bin/perch.js"
# Set by the script rather than left to npm's bin-links at install time.
executable "the launcher is executable" "$out/perch-cli/bin/perch.js"

# Matched on content rather than on existence, because an empty file passes the
# weaker check and an empty npm page is what this is here to prevent — and the
# path this reaches by is the one line that moving the script could break.
same "the wrapper holds the repository's README" "$out/perch-cli/README.md" "$readme"

# ------------------------------------------------------------ and the refusal

# Short of its three arguments the script refuses, rather than writing whatever
# part of the tree it can work out. Asserted on the exit status directly, which
# is the one thing a caller in a workflow reads.
short="$root/short"
mkdir -p "$short"
if (cd "$short" && node "$build" "$version" "$bins") >/dev/null 2>&1; then
    status=zero
else
    status=non-zero
fi
equals "build.mjs refuses without an out directory" "$status" non-zero
equals "and wrote nothing before refusing" "$(count "$short")" 0

report
