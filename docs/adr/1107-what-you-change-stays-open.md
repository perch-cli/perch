# What you change stays open

Perch is licensed under the GNU General Public License, version 3 or any later
version, with one additional term under its section 7 reserving the name. It
was `MIT OR Apache-2.0`, the pair the Rust ecosystem uses, from the first push
until this decision.

## What Perch is trusted with

Perch holds a person's Credentials and decides, on their behalf, which Account
is spent. What makes that tolerable is that anyone running it can read what it
does: the keychain module says where a Credential goes, the Ranking says why
this Account and not that one, and a reader with the tree can check both. A
modified Perch shipped closed asks for the same trust and takes the reading
away.

GPLv3 attaches the right to read to every copy that gets passed on. Use it,
sell it, change it, but whoever receives the changed one receives its source
too. That is the property the license is chosen for, and the permissive pair
does not have it: under MIT or Apache-2.0 a fork may ship a binary that holds
Credentials, and nobody who runs it may ask what it does with them.

## The live alternative

`MIT OR Apache-2.0` stays live, and is the right answer for a different kind of
project. A crate meant to be linked into other people's programs is used under
whatever license those programs carry, and a copyleft crate is one most of them
cannot take. Perch is not published as a crate, and crates.io is not a Channel
(ADR this-repo-assembles-a-release). Its library target exists so the binary
and the tests share code, and promises no public API; nothing links Perch.

The pair's own case, that MIT carries no patent grant and Apache-2.0 cannot be
combined with GPLv2, is a case about being linked. GPLv3 carries an express
patent grant of its own, in section 11, so nothing that case protected is lost.

## What it costs

A permissive project cannot lift a module out of Perch. The seams are clean by
design (ADR code-lives-where-it-reaches) and `src/keychain.rs` is the piece
most worth lifting; under this license it goes only to a project that makes the
same promise. That is the trade. The module is worth more inside a program
whose every copy can be read than in one that cannot be.

What it does not cost is a Channel. GitHub Releases, Homebrew and npm all carry
GPL software. Every dependency in the tree offers at least one GPLv3-compatible
license: MIT, Apache-2.0, a BSD clause set, Unlicense or Unicode-3.0, and one
crate offers `Apache-2.0 OR GPL-2.0-only`. Read from `cargo metadata` by hand;
a tool for the check is a decision this document does not make.

Nor does it reach across a process. Perch invokes Claude Code as a subprocess
and is invoked from shell prompts, and the license binds neither side of either
boundary: a program that runs Perch is not a work based on Perch, and Perch is
not a work based on what it runs.

## Or later

"Version 3 or any later version" rather than version 3 alone, because a future
GPL version is adopted without collecting consent from every contributor. After
the first outside contribution lands, a relicense needs every author's yes, and
that is the one thing this decision cannot get back later. The Free Software
Foundation is the only body that can publish a later version, and section 14
binds it to preserve the license's spirit.

## The name

The scenario the license guards against is a rebranded Perch: the same binary
under another name, or a changed one under this name, asking for trust the
project never extended. Apache-2.0 reserves the name in its section 6, and
GPLv3 does not on its own. Two of section 7's six categories do the work.
Section 7(e) permits a term "declining to grant rights under trademark law for
use of some trade names, trademarks, or service marks", and 7(c) one
"requiring that modified versions of such material be marked in reasonable
ways as different from the original version". `ADDITIONAL-TERMS` is one clause
of each: the names and any logo may not identify a modified version without
permission, and a modified version must say it is one. A term outside those
six categories is one section 7 lets a recipient strip, which is why the file
names both.

The term ships in every Artifact beside the license, because section 7 requires
additional terms to be placed in the relevant source files or in a notice
stating where to find them, and an archive that carries one file of the pair
and not the other has a license a reader cannot reconstruct. The SPDX
expression a package registry displays cannot carry a section 7 term, so the
term is read from the archive on every Channel that hands one over, and from
the README, which quotes it, in the npm package that carries only the binary.

## What moves, and what does not

One license file holding the GPLv3 text verbatim, and one holding the
additional term. The SPDX expression `GPL-3.0-or-later` in the crate manifest,
the npm package and the Homebrew formula. Every archive carries both files
where it carried two before, and `CONTEXT.md`'s Artifact says so.

No command, flag, exit code or `--json` shape moves, so the entry lands under
`### Changed`.

A contribution arrives under these terms because `CONTRIBUTING.md` says so and
a pull request is the act of accepting them. GPLv3 has no equivalent of
Apache-2.0 section 5, which placed a contribution under the license on its own,
so that statement is now the whole mechanism, and the decision to run without a
CLA (ADR a-gate-lives-outside-the-tree) stands on it.
