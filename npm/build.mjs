// Assembles the six npm packages a release publishes: one `perch-cli` wrapper,
// and one platform package per target holding a single executable.
//
//   node npm/build.mjs <version> <binaries-dir> <out-dir>
//
// <binaries-dir> holds one directory per Rust target, each containing the
// executable extracted from that target's release archive.
//
// The versions in npm/perch-cli/package.json are all `0.0.0`. They are written
// here rather than kept in the file because they have to agree exactly — the
// wrapper depends on its platform packages by exact version, so a mismatch is
// an install that resolves to a binary from a different release. A number that
// has to be right in six places is a number no one should be typing.

import { chmodSync, copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));

// `os` and `cpu` are what npm resolves against: it installs the one optional
// dependency whose pair matches the machine, and quietly skips the rest.
//
// No `libc` field on the Linux packages, deliberately. They are built against
// musl and linked statically, which means they run on glibc systems too —
// declaring `libc: ["musl"]` would be npm refusing to install a binary that
// works perfectly well.
const TARGETS = [
  { target: "aarch64-apple-darwin", pkg: "@perch-cli/darwin-arm64", os: "darwin", cpu: "arm64" },
  { target: "x86_64-apple-darwin", pkg: "@perch-cli/darwin-x64", os: "darwin", cpu: "x64" },
  { target: "aarch64-unknown-linux-musl", pkg: "@perch-cli/linux-arm64", os: "linux", cpu: "arm64" },
  { target: "x86_64-unknown-linux-musl", pkg: "@perch-cli/linux-x64", os: "linux", cpu: "x64" },
  { target: "x86_64-pc-windows-msvc", pkg: "@perch-cli/win32-x64", os: "win32", cpu: "x64" },
];

const [version, binariesDir, outDir] = process.argv.slice(2);
if (!version || !binariesDir || !outDir) {
  console.error("usage: node npm/build.mjs <version> <binaries-dir> <out-dir>");
  process.exit(1);
}

const wrapper = JSON.parse(readFileSync(join(HERE, "perch-cli", "package.json"), "utf8"));

for (const { target, pkg, os, cpu } of TARGETS) {
  const exe = os === "win32" ? "perch.exe" : "perch";
  const dir = join(outDir, pkg.replace("/", "-"));
  mkdirSync(join(dir, "bin"), { recursive: true });

  copyFileSync(join(binariesDir, target, exe), join(dir, "bin", exe));
  // npm preserves the mode it finds, and what it finds after an unzip on a
  // Windows runner is not executable. Set it here, where every platform's
  // package is assembled the same way.
  chmodSync(join(dir, "bin", exe), 0o755);

  writeFileSync(
    join(dir, "package.json"),
    `${JSON.stringify(
      {
        name: pkg,
        version,
        description: `The ${os}-${cpu} binary for perch-cli`,
        homepage: wrapper.homepage,
        repository: wrapper.repository,
        license: wrapper.license,
        author: wrapper.author,
        os: [os],
        cpu: [cpu],
        // No `exports`, so the wrapper can resolve `<pkg>/package.json` and
        // walk from there to the executable beside it.
        files: [`bin/${exe}`],
      },
      null,
      2,
    )}\n`,
  );

  console.log(`${pkg}@${version} from ${target}`);
}

const dir = join(outDir, "perch-cli");
mkdirSync(join(dir, "bin"), { recursive: true });
copyFileSync(join(HERE, "perch-cli", "bin", "perch.js"), join(dir, "bin", "perch.js"));
// npm renders this on the package page, and a page with nothing on it is a
// poor advertisement for a tool asking to hold your credentials.
copyFileSync(join(HERE, "..", "README.md"), join(dir, "README.md"));

wrapper.version = version;
for (const { pkg } of TARGETS) {
  wrapper.optionalDependencies[pkg] = version;
}
writeFileSync(join(dir, "package.json"), `${JSON.stringify(wrapper, null, 2)}\n`);

console.log(`perch-cli@${version}`);
