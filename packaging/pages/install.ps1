# Installs Perch on Windows.
#
#   irm https://perch-cli.github.io/perch/install.ps1 | iex
#
# Environment:
#   PERCH_VERSION      a tag to install, such as v0.1.0. Default: the latest release.
#   PERCH_INSTALL_DIR  where to put the binary. Default: %LOCALAPPDATA%\Perch\bin.

$ErrorActionPreference = "Stop"

$Repo = "perch-cli/perch"

# Overridable so the script can be tested against a fabricated release served
# locally. Nothing in normal use should set these.
$Api = if ($env:PERCH_API_BASE) { $env:PERCH_API_BASE } else { "https://api.github.com/repos/$Repo" }
$Downloads = if ($env:PERCH_DOWNLOAD_BASE) { $env:PERCH_DOWNLOAD_BASE } else { "https://github.com/$Repo/releases/download" }

$InstallDir = if ($env:PERCH_INSTALL_DIR) { $env:PERCH_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Perch\bin" }

function Say($message) { Write-Host "perch: $message" }
function Die($message) { Write-Error "perch: $message"; exit 1 }

# ---------------------------------------------------------------- which build

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
if ($arch -ne "X64") {
    Die "no Perch build for Windows on $arch. Only x64 is published; on an Arm machine, x64 emulation is not something Perch has been tested under."
}
$target = "x86_64-pc-windows-msvc"

# -------------------------------------------------------------- which version

if ($env:PERCH_VERSION) {
    $version = $env:PERCH_VERSION
}
else {
    try {
        $version = (Invoke-RestMethod -Uri "$Api/releases/latest").tag_name
    }
    catch {
        Die "could not work out the latest release: $_"
    }
}
if (-not $version) { Die "could not work out the latest release" }

$archive = "perch-$version-$target.zip"
Say "installing $version for $target"

# ------------------------------------------------------------------- download

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("perch-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmp | Out-Null

try {
    try {
        Invoke-WebRequest -Uri "$Downloads/$version/$archive" -OutFile (Join-Path $tmp $archive)
    }
    catch {
        Die "could not download $archive — is $version a release?"
    }

    try {
        Invoke-WebRequest -Uri "$Downloads/$version/SHA256SUMS" -OutFile (Join-Path $tmp "SHA256SUMS")
    }
    catch {
        Die "could not download SHA256SUMS for $version"
    }

    # ----------------------------------------------------------------- verify

    $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $archive)).Hash.ToLower()

    $expected = $null
    foreach ($line in Get-Content (Join-Path $tmp "SHA256SUMS")) {
        # `<hash>  <filename>`, as sha256sum writes it.
        $fields = $line -split '\s+', 2
        if ($fields.Count -eq 2 -and $fields[1].Trim() -eq $archive) {
            $expected = $fields[0].ToLower()
            break
        }
    }
    if (-not $expected) { Die "SHA256SUMS for $version does not mention $archive" }

    if ($actual -ne $expected) {
        Die "checksum mismatch for ${archive}: expected $expected, got $actual"
    }
    Say "checksum ok"

    # The same bargain as install.sh: attempted only when `gh` is installed and
    # logged in, and then binding rather than advisory. The download already
    # succeeded, so a failure from here is saying something about the file.
    $gh = Get-Command gh -ErrorAction SilentlyContinue
    if ($gh) {
        gh auth status *> $null
        if ($LASTEXITCODE -eq 0) {
            gh attestation verify (Join-Path $tmp $archive) --repo $Repo *> $null
            if ($LASTEXITCODE -eq 0) {
                Say "provenance ok — built by $Repo"
            }
            else {
                Die "provenance check failed for $archive. It does not appear to have been built by $Repo. Not installing."
            }
        }
    }

    # ---------------------------------------------------------------- install

    Expand-Archive -Path (Join-Path $tmp $archive) -DestinationPath (Join-Path $tmp "unpacked") -Force
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

    $destination = Join-Path $InstallDir "perch.exe"
    $superseded = "$destination.old"

    # Windows holds a running executable open, so `perch.exe` cannot be
    # written over while it is running — which is exactly the state `perch
    # upgrade` runs this in, since the Perch being replaced is the one that
    # started this script (ADR 0039). What Windows *does* allow is renaming a
    # running executable, so the old one is moved aside and the new one takes
    # its name.
    #
    # Cleared first because a rename onto an occupied name fails, and the
    # previous upgrade will have left one: the binary was still running when it
    # got here, so nothing could delete it at the time.
    Remove-Item -Force $superseded -ErrorAction SilentlyContinue
    $movedAside = $false
    if (Test-Path $destination) {
        try {
            Move-Item -Force $destination $superseded
            $movedAside = $true
        }
        catch {
            Die "could not move $destination aside: $_"
        }
    }

    try {
        Move-Item -Force (Join-Path $tmp "unpacked\perch.exe") $destination
    }
    catch {
        # Put back the one that was working. A failed upgrade leaving the
        # machine with no Perch at all is a worse outcome than the upgrade not
        # happening, and this is the only window in which that is possible.
        if ($movedAside) { Move-Item -Force $superseded $destination }
        Die "could not write $destination — $_"
    }

    # Best effort: it is still running if this was an upgrade, in which case
    # this fails and the next upgrade clears it above. `perch.exe.old` sitting
    # beside the binary is the Installation's litter rather than something Perch
    # holds, which is the side of the line ADR 0033 drew that it belongs on.
    if ($movedAside) {
        Remove-Item -Force $superseded -ErrorAction SilentlyContinue
    }

    Say "installed to $destination"
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

# A user PATH entry is a registry value, so the exact segment that was added
# can be taken back out again later — which is what makes writing one here
# defensible where appending to an rc file on Unix is not (ADR 0033).
#
# Both the user and the machine PATH are consulted, and each is matched a
# segment at a time rather than as a substring: a machine-wide entry is still
# on the PATH, and an existing `C:\foo\binary` is not a `C:\foo\bin`. Getting
# that right is what makes a re-install neither ask again nor add the directory
# twice — and a re-install is exactly what `perch upgrade` performs on an
# installer Installation, by running this script (ADR 0039).
function Test-OnPath($directory) {
    $wanted = $directory.TrimEnd('\')
    foreach ($scope in "User", "Machine") {
        $value = [Environment]::GetEnvironmentVariable("Path", $scope)
        if (-not $value) { continue }
        foreach ($entry in $value -split ';') {
            $entry = $entry.Trim().TrimEnd('\')
            if ($entry -and $entry -ieq $wanted) { return $true }
        }
    }
    return $false
}

if (-not (Test-OnPath $InstallDir)) {
    $command = "[Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ';$InstallDir', 'User')"

    # `irm ... | iex` runs in the session the user is standing in, so a prompt
    # here reaches a real console. A scripted install has nobody to ask and
    # stays inert.
    if ([Environment]::UserInteractive) {
        Say ""
        $typed = Read-Host "perch: $InstallDir is not on your PATH. Add it for future sessions? [Y/n]"
        # Everything but a plain no is a yes, including a bare Return, which is
        # how `perch purge` asks about an Export. End of input is a no there
        # too, and Read-Host answers $null for it — nobody was there to be
        # asked, and this is the branch that writes.
        $answered = if ($null -eq $typed) { "n" } else { $typed.Trim().ToLower() }
        if ($answered -eq "n" -or $answered -eq "no") {
            Say "left your PATH alone. Add it later with:"
            Say "    $command"
        }
        else {
            $current = [Environment]::GetEnvironmentVariable("Path", "User")
            $updated = if ($current) { $current.TrimEnd(';') + ";$InstallDir" } else { $InstallDir }
            [Environment]::SetEnvironmentVariable("Path", $updated, "User")
            # .NET broadcasts WM_SETTINGCHANGE, so new processes see this — but
            # Windows resolves a command against the PATH of the process that
            # started it, and the console being typed at started before now.
            Say "added $InstallDir to your user PATH — reopen your terminal to pick it up."
            Say "Perch's install guide says how to take it back out:"
            Say "    https://github.com/perch-cli/perch/blob/main/docs/guide/installing.md"
        }
    }
    else {
        Say ""
        Say "$InstallDir is not on your PATH. Add it for future sessions with:"
        Say "    $command"
        Say "and reopen your terminal."
    }
}

Say "run 'perch status' to see where you are"
