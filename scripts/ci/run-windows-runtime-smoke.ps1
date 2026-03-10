param(
    [string]$ArtifactPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$RootDir = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$SmokeDir = Join-Path $RootDir 'voicewin-tauri/src-tauri/target/ci-smoke/windows-runtime'
$StdoutLog = Join-Path $SmokeDir 'stdout.log'
$StderrLog = Join-Path $SmokeDir 'stderr.log'
$EvidenceLog = Join-Path $SmokeDir 'voicewin.log'
$TargetFile = Join-Path $SmokeDir 'notepad-runtime-target.txt'
$Transcript = 'VoiceWin runtime smoke transcript'
$ProvenancePattern = '^VoiceWin startup: version=.* git_sha=.*$'
$StartPattern = '^VOICEWIN_RUNTIME_SMOKE_START version=.* git_sha=.*$'
$SuccessPattern = '^VOICEWIN_RUNTIME_SMOKE_OK version=.* git_sha=.*$'
$DefaultArtifactCandidates = @(
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/x86_64-pc-windows-msvc/release/VoiceWin.exe'),
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/x86_64-pc-windows-msvc/release/voicewin-tauri.exe'),
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/release/VoiceWin.exe'),
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/release/voicewin-tauri.exe')
)

$Shell = New-Object -ComObject WScript.Shell

function Wait-ForWindow {
    param(
        [Parameter(Mandatory = $true)]
        [System.Diagnostics.Process]$Process
    )

    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        $Process.Refresh()
        if ($Process.MainWindowHandle -ne 0) {
            return
        }

        Start-Sleep -Milliseconds 200
    }

    throw "Timed out waiting for a window for process id $($Process.Id)."
}

function Activate-ProcessWindow {
    param(
        [Parameter(Mandatory = $true)]
        [int]$ProcessId,

        [int]$Attempts = 10
    )

    $activated = $false
    for ($attempt = 0; $attempt -lt $Attempts; $attempt++) {
        $activated = $Shell.AppActivate($ProcessId)
        Start-Sleep -Milliseconds 150
    }

    if (-not $activated) {
        throw "Could not activate process window for pid $ProcessId."
    }
}

function Hold-TargetFocusWhileRunning {
    param(
        [Parameter(Mandatory = $true)]
        [System.Diagnostics.Process]$Process,

        [Parameter(Mandatory = $true)]
        [int]$TargetProcessId,

        [int]$MaxSeconds = 30
    )

    $deadline = (Get-Date).AddSeconds($MaxSeconds)
    while ((Get-Date) -lt $deadline) {
        $Process.Refresh()
        if ($Process.HasExited) {
            return
        }

        [void]$Shell.AppActivate($TargetProcessId)
        Start-Sleep -Milliseconds 250
    }
}

function Find-SmokeOutputFile {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Paths
    )

    foreach ($path in $Paths) {
        if (-not (Test-Path -LiteralPath $path)) {
            continue
        }

        $provenanceMatch = Select-String -Path $path -Pattern $ProvenancePattern
        $startMatch = Select-String -Path $path -Pattern $StartPattern
        $successMatch = Select-String -Path $path -Pattern $SuccessPattern
        if (-not $provenanceMatch -or -not $startMatch -or -not $successMatch) {
            continue
        }

        if ($provenanceMatch[0].LineNumber -ge $startMatch[0].LineNumber) {
            throw "Startup provenance line did not appear before runtime smoke start marker in Windows process output: $path"
        }

        if ($startMatch[0].LineNumber -ge $successMatch[0].LineNumber) {
            throw "Runtime smoke start marker did not appear before success marker in Windows process output: $path"
        }

        return $path
    }

    return $null
}

New-Item -ItemType Directory -Force -Path $SmokeDir | Out-Null
Remove-Item $StdoutLog, $StderrLog, $EvidenceLog, $TargetFile -ErrorAction SilentlyContinue
[System.IO.File]::WriteAllText($TargetFile, '', [System.Text.Encoding]::UTF8)

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
    throw "Windows runtime smoke artifact not found: $ArtifactPath"
}

$notepad = $null
$process = $null
$previousRuntimeSmokeEnv = $env:VOICEWIN_RUNTIME_SMOKE_TEST
$previousTranscriptEnv = $env:VOICEWIN_RUNTIME_SMOKE_TRANSCRIPT
$previousExpectEnv = $env:VOICEWIN_RUNTIME_SMOKE_EXPECT_PROCESS

try {
    $notepad = Start-Process -FilePath 'notepad.exe' -ArgumentList @($TargetFile) -PassThru
    Wait-ForWindow -Process $notepad
    Activate-ProcessWindow -ProcessId $notepad.Id -Attempts 8

    Write-Host "Launching Windows runtime smoke executable: $ArtifactPath"
    $env:VOICEWIN_RUNTIME_SMOKE_TEST = '1'
    $env:VOICEWIN_RUNTIME_SMOKE_TRANSCRIPT = $Transcript
    $env:VOICEWIN_RUNTIME_SMOKE_EXPECT_PROCESS = 'notepad.exe'

    $process = Start-Process -FilePath $ArtifactPath -PassThru -RedirectStandardOutput $StdoutLog -RedirectStandardError $StderrLog
    $runtimeTimer = [System.Diagnostics.Stopwatch]::StartNew()
    Hold-TargetFocusWhileRunning -Process $process -TargetProcessId $notepad.Id -MaxSeconds 30
    $process.Refresh()

    if (-not $process.HasExited) {
        $remainingMs = [Math]::Max(0, 30000 - [int]$runtimeTimer.ElapsedMilliseconds)
        if (-not $process.WaitForExit($remainingMs)) {
            try {
                $process.Kill($true)
            }
            catch {
            }

            throw 'Windows runtime smoke app did not exit within 30 seconds.'
        }
    }

    if ($process.ExitCode -ne 0) {
        try {
            $runtimeTimer.Stop()
        }
        catch {
        }

        throw "Windows runtime smoke app exited with code $($process.ExitCode)."
    }

    Activate-ProcessWindow -ProcessId $notepad.Id -Attempts 4
    $Shell.SendKeys('^s')
    Start-Sleep -Milliseconds 500

    $actualText = $null
    for ($attempt = 0; $attempt -lt 20; $attempt++) {
        $actualText = [System.IO.File]::ReadAllText($TargetFile)
        if ($actualText -eq $Transcript) {
            break
        }

        Start-Sleep -Milliseconds 200
    }

    if ($actualText -ne $Transcript) {
        throw "Notepad file contents mismatch. Expected '$Transcript' but found '$actualText'."
    }

    $outputPath = Find-SmokeOutputFile -Paths @($StdoutLog, $StderrLog)
    if (-not $outputPath) {
        throw "Runtime smoke provenance/start/success markers not found in Windows process output. Stdout: $StdoutLog Stderr: $StderrLog"
    }

    Copy-Item -LiteralPath $outputPath -Destination $EvidenceLog -Force
    Select-String -Path $EvidenceLog -Pattern $SuccessPattern
    Write-Host "Runtime smoke markers found in process output: $outputPath"
    Write-Host "Notepad target contents matched transcript: $TargetFile"
    Write-Host "Runtime smoke evidence copied to $EvidenceLog"
}
finally {
    if ($null -eq $previousRuntimeSmokeEnv) {
        Remove-Item Env:VOICEWIN_RUNTIME_SMOKE_TEST -ErrorAction SilentlyContinue
    }
    else {
        $env:VOICEWIN_RUNTIME_SMOKE_TEST = $previousRuntimeSmokeEnv
    }

    if ($null -eq $previousTranscriptEnv) {
        Remove-Item Env:VOICEWIN_RUNTIME_SMOKE_TRANSCRIPT -ErrorAction SilentlyContinue
    }
    else {
        $env:VOICEWIN_RUNTIME_SMOKE_TRANSCRIPT = $previousTranscriptEnv
    }

    if ($null -eq $previousExpectEnv) {
        Remove-Item Env:VOICEWIN_RUNTIME_SMOKE_EXPECT_PROCESS -ErrorAction SilentlyContinue
    }
    else {
        $env:VOICEWIN_RUNTIME_SMOKE_EXPECT_PROCESS = $previousExpectEnv
    }

    if ($notepad -and -not $notepad.HasExited) {
        try {
            Activate-ProcessWindow -ProcessId $notepad.Id -Attempts 2
            $notepad.CloseMainWindow() | Out-Null
            Start-Sleep -Milliseconds 500
        }
        catch {
        }

        if (-not $notepad.HasExited) {
            Stop-Process -Id $notepad.Id -Force -ErrorAction SilentlyContinue
        }
    }
}
