#!/usr/bin/env node
"use strict";

// The whole of the npm package. `perch-cli` carries no binary itself: it
// declares one optional dependency per platform and npm installs the one whose
// `os` and `cpu` match, so this file finds it and gets out of the way. No
// postinstall script anywhere, which is the point of doing it this way
// (ADR this-repo-assembles-a-release).

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

// Ignored, because the terminal delivered it to everything in the foreground
// group already; passed on where it is directed at this pid alone. `spawn`
// rather than `spawnSync` too: a blocked event loop forwards nothing.
process.on("SIGINT", () => {});
for (const signal of ["SIGTERM", "SIGHUP"]) {
  process.on(signal, () => child.kill(signal));
}

child.on("error", (error) => {
  fail(`could not run ${binary}: ${error.message}`);
});

// `perch run` exits with whatever the client it launched exited with, so this
// passes the number through untouched — including the shell's convention for a
// process a signal killed.
child.on("close", (status, signal) => {
  if (signal) {
    const number = require("node:os").constants.signals[signal];
    process.exit(number ? 128 + number : 1);
  }
  process.exit(status === null ? 1 : status);
});
