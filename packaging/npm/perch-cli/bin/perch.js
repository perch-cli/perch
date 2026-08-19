#!/usr/bin/env node
"use strict";

// The whole of the npm package. `perch-cli` carries no binary itself: it
// declares one optional dependency per platform, each holding exactly one
// executable, and npm installs the single one whose `os` and `cpu` match. This
// file finds it and gets out of the way.
//
// No postinstall script anywhere, which is the point of doing it this way. A
// package that downloads its binary when it is installed does not work under
// `npm ci --ignore-scripts`, behind a proxy, or offline — and for a program
// that holds credentials, "npm has the bytes, with provenance" is a better
// answer than "npm ran a script that fetched something".

const { spawn } = require("node:child_process");
const path = require("node:path");

const PACKAGES = {
  "darwin arm64": "@perch-cli/darwin-arm64",
  "darwin x64": "@perch-cli/darwin-x64",
  "linux arm64": "@perch-cli/linux-arm64",
  "linux x64": "@perch-cli/linux-x64",
  "win32 x64": "@perch-cli/win32-x64",
};

function fail(message) {
  // Exit code 1 is Perch's "something else went wrong". Everything above it is
  // a specific refusal Perch makes about accounts, and none of them are this.
  process.stderr.write(`perch: ${message}\n`);
  process.exit(1);
}

const platform = `${process.platform} ${process.arch}`;
const pkg = PACKAGES[platform];
if (!pkg) {
  fail(
    `there is no Perch build for ${platform}. ` +
      `Supported: ${Object.keys(PACKAGES).join(", ")}.`,
  );
}

let binary;
try {
  // Resolve the package's manifest and walk from there, rather than resolving
  // the executable directly: the manifest is the one path a package is always
  // willing to hand out.
  const manifest = require.resolve(`${pkg}/package.json`);
  binary = path.join(
    path.dirname(manifest),
    "bin",
    process.platform === "win32" ? "perch.exe" : "perch",
  );
} catch {
  fail(
    `${pkg} is not installed. It is an optional dependency of perch-cli, so ` +
      `an install run with --no-optional, or one that failed quietly, leaves ` +
      `it out. Reinstall perch-cli.`,
  );
}

const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });

// A terminal delivers SIGINT to every process in the foreground group, so both
// this process and perch already have it. Left alone, Node's default action
// would kill this one immediately — while perch is still putting the terminal
// back out of raw mode. Ignoring it here leaves perch as the only thing acting
// on it, and it is the one holding the screen.
//
// A *directed* signal is the other case, and it reaches this pid alone: perch is
// a separate process, not a separate group, so nothing delivers it there. That
// is `timeout 30 npx perch watcher run`, a CI runner canceling a job, and a
// terminal closing on a detached shell. Ignored rather than forwarded, the pair
// ran until perch decided to stop on its own and `timeout` was defeated — so
// those are passed on. Async `spawn` rather than `spawnSync` for the same
// reason: a blocked event loop cannot forward anything, so the handlers could
// not have run even in principle.
process.on("SIGINT", () => {});
for (const signal of ["SIGTERM", "SIGHUP"]) {
  process.on(signal, () => child.kill(signal));
}

child.on("error", (error) => {
  fail(`could not run ${binary}: ${error.message}`);
});

// `perch run` exits with whatever the client it launched exited with, so a
// script wrapping perch reads the program's own code. That only stays true if
// this passes the number through untouched — including the shell's convention
// for a process a signal killed.
child.on("close", (status, signal) => {
  if (signal) {
    const number = require("node:os").constants.signals[signal];
    process.exit(number ? 128 + number : 1);
  }
  process.exit(status === null ? 1 : status);
});
