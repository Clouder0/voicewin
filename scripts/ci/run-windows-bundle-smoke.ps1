param(
    [string]$ArtifactPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$RootDir = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$SmokeDir = Join-Path $RootDir 'voicewin-tauri/src-tauri/target/ci-smoke/windows'
$StdoutLog = Join-Path $SmokeDir 'stdout.log'
$StderrLog = Join-Path $SmokeDir 'stderr.log'
$EvidenceLog = Join-Path $SmokeDir 'voicewin.log'
$ProvenancePattern = '^VoiceWin startup: version=.* git_sha=.*$'
$MarkerPattern = '^VOICEWIN_SMOKE_OK version=.* git_sha=.*$'
$DefaultArtifactCandidates = @(
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/x86_64-pc-windows-msvc/release/VoiceWin.exe'),
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/x86_64-pc-windows-msvc/release/voicewin-tauri.exe'),
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/release/VoiceWin.exe'),
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/release/voicewin-tauri.exe')
)

New-Item -ItemType Directory -Force -Path $SmokeDir | Out-Null
Remove-Item $StdoutLog, $StderrLog, $EvidenceLog -ErrorAction SilentlyContinue

if (-not $ArtifactPath) {
    foreach ($candidate in $DefaultArtifactCandidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            $ArtifactPath = $candidate
            break
        }
    }

    if (-not $ArtifactPath) {
        throw "Could not locate a built Windows release executable. Expected one of: $($DefaultArtifactCandidates -join ', ')"
    }
}

if (-not (Test-Path -LiteralPath $ArtifactPath -PathType Leaf)) {
    throw "Windows smoke artifact not found: $ArtifactPath"
}

function Copy-EvidenceFrom {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    Copy-Item -LiteralPath $Path -Destination $EvidenceLog -Force
    Select-String -Path $EvidenceLog -Pattern $MarkerPattern
    Write-Host "Smoke evidence copied to $EvidenceLog"
}

function Test-SmokeOutputFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    if (-not (Test-Path -LiteralPath $Path)) {
        return $false
    }

    $provenanceMatch = Select-String -Path $Path -Pattern $ProvenancePattern
    if (-not $provenanceMatch) {
        return $false
    }

    $markerMatch = Select-String -Path $Path -Pattern $MarkerPattern
    if (-not $markerMatch) {
        return $false
    }

    if ($provenanceMatch[0].LineNumber -ge $markerMatch[0].LineNumber) {
        throw "Startup provenance line did not appear before smoke marker in Windows process output: $Path"
    }

    return $true
}

Write-Host "Launching Windows smoke executable: $ArtifactPath"
$previousSmokeEnv = $env:VOICEWIN_SMOKE_TEST
$env:VOICEWIN_SMOKE_TEST = '1'

try {
    $process = Start-Process -FilePath $ArtifactPath -PassThru -RedirectStandardOutput $StdoutLog -RedirectStandardError $StderrLog
    if (-not $process.WaitForExit(30000)) {
        try {
            $process.Kill($true)
        }
        catch {
        }

        throw 'Windows smoke app did not exit within 30 seconds.'
    }

    if ($process.ExitCode -ne 0) {
        throw "Windows smoke app exited with code $($process.ExitCode)."
    }
}
finally {
    if ($null -eq $previousSmokeEnv) {
        Remove-Item Env:VOICEWIN_SMOKE_TEST -ErrorAction SilentlyContinue
    }
    else {
        $env:VOICEWIN_SMOKE_TEST = $previousSmokeEnv
    }
}

if (Test-SmokeOutputFile -Path $StdoutLog) {
    Write-Host "Smoke marker found in process output: $StdoutLog"
    Copy-EvidenceFrom -Path $StdoutLog
    exit 0
}

if (Test-SmokeOutputFile -Path $StderrLog) {
    Write-Host "Smoke marker found in process error output: $StderrLog"
    Copy-EvidenceFrom -Path $StderrLog
    exit 0
}

throw "Startup provenance line and smoke marker not found in Windows process output. Stdout: $StdoutLog Stderr: $StderrLog"
