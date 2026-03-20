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
$ResolvedArtifactLog = Join-Path $SmokeDir 'resolved-artifact.txt'
$InstallLayoutLog = Join-Path $SmokeDir 'install-layout.txt'
$AppLogCandidatesLog = Join-Path $SmokeDir 'app-log-candidates.txt'
$AppLogEvidence = Join-Path $SmokeDir 'app.log'
$Transcript = 'VoiceWin runtime smoke transcript'
$ProvenancePattern = '^VoiceWin startup: version=.* git_sha=.*$'
$StartPattern = '^VOICEWIN_RUNTIME_SMOKE_START version=.* git_sha=.*$'
$SuccessPattern = '^VOICEWIN_RUNTIME_SMOKE_OK version=.* git_sha=.*$'
$FailurePattern = '^VOICEWIN_RUNTIME_SMOKE_FAIL version=.* git_sha=.* reason=.*$'
$DefaultInstallerDirectories = @(
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis'),
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/release/bundle/nsis')
)
$DefaultExecutableCandidates = @(
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/x86_64-pc-windows-msvc/release/VoiceWin.exe'),
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/x86_64-pc-windows-msvc/release/voicewin-tauri.exe'),
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/release/VoiceWin.exe'),
    (Join-Path $RootDir 'voicewin-tauri/src-tauri/target/release/voicewin-tauri.exe')
)
$InstallDir = Join-Path $env:TEMP ("voicewin-runtime-smoke-install-" + [Guid]::NewGuid().ToString('N'))

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

function Assert-NoFailureMarker {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Paths
    )

    foreach ($path in $Paths) {
        if (-not (Test-Path -LiteralPath $path)) {
            continue
        }

        $failureMatch = Select-String -Path $path -Pattern $FailurePattern
        if (-not $failureMatch) {
            continue
        }

        Copy-Item -LiteralPath $path -Destination $EvidenceLog -Force
        throw "Runtime smoke failure marker found in Windows process output: $path :: $($failureMatch[0].Line)"
    }
}

function Find-PackagedInstallerArtifact {
    foreach ($dir in $DefaultInstallerDirectories) {
        if (-not (Test-Path -LiteralPath $dir -PathType Container)) {
            continue
        }

        $installer = Get-ChildItem -LiteralPath $dir -Filter '*-setup.exe' -File |
            Sort-Object LastWriteTimeUtc -Descending |
            Select-Object -First 1
        if ($installer) {
            return $installer.FullName
        }
    }

    return $null
}

function Try-Find-InstalledExecutable {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InstallRoot
    )

    $candidates = @(
        (Join-Path $InstallRoot 'VoiceWin.exe'),
        (Join-Path $InstallRoot 'voicewin-tauri.exe')
    )

    if (Test-Path -LiteralPath $InstallRoot -PathType Container) {
        $candidates += Get-ChildItem -LiteralPath $InstallRoot -Recurse -Filter *.exe -File |
            Where-Object { $_.Name -notmatch '^(uninstall|uninst|setup).*$' } |
            Sort-Object FullName |
            ForEach-Object { $_.FullName }
    }

    foreach ($candidate in ($candidates | Select-Object -Unique)) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return $candidate
        }
    }

    return $null
}

function Write-InstallLayoutEvidence {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InstallRoot
    )

    if (-not (Test-Path -LiteralPath $InstallRoot -PathType Container)) {
        "install root missing: $InstallRoot" | Set-Content -LiteralPath $InstallLayoutLog -Encoding utf8
        return
    }

    Get-ChildItem -LiteralPath $InstallRoot -Recurse -Force |
        Sort-Object FullName |
        ForEach-Object {
            if ($_.PSIsContainer) {
                "[dir] $($_.FullName)"
            }
            else {
                "[file] $($_.FullName) size=$($_.Length)"
            }
        } | Set-Content -LiteralPath $InstallLayoutLog -Encoding utf8
}

function Install-PackagedRuntimeSmokeArtifact {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InstallerPath
    )

    if (Test-Path -LiteralPath $InstallDir) {
        Remove-Item -LiteralPath $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Write-Host "Installing Windows runtime smoke artifact silently: $InstallerPath"
    Write-Host "Windows runtime smoke install dir: $InstallDir"

    $installerProcess = Start-Process -FilePath $InstallerPath -ArgumentList @('/S', "/D=$InstallDir") -PassThru -Wait
    if ($installerProcess.ExitCode -ne 0) {
        throw "Windows runtime smoke installer exited with code $($installerProcess.ExitCode): $InstallerPath"
    }

    $resolvedExecutable = $null
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        $resolvedExecutable = Try-Find-InstalledExecutable -InstallRoot $InstallDir
        if ($resolvedExecutable) {
            break
        }

        Start-Sleep -Milliseconds 250
    }

    Write-InstallLayoutEvidence -InstallRoot $InstallDir

    if (-not $resolvedExecutable) {
        throw "Installed Windows runtime smoke executable not found under $InstallDir. See $InstallLayoutLog"
    }

    Write-Host "Windows runtime smoke installed executable: $resolvedExecutable"
    return $resolvedExecutable
}

function Resolve-SmokeExecutablePath {
    param(
        [string]$RequestedArtifactPath
    )

    $selectedArtifactPath = $RequestedArtifactPath
    if (-not $selectedArtifactPath) {
        $selectedArtifactPath = Find-PackagedInstallerArtifact
    }

    if (-not $selectedArtifactPath) {
        foreach ($candidate in $DefaultExecutableCandidates) {
            if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                $selectedArtifactPath = $candidate
                break
            }
        }
    }

    if (-not $selectedArtifactPath) {
        throw "Could not locate a built Windows runtime smoke artifact. Expected NSIS installer under $($DefaultInstallerDirectories -join ', ') or one of: $($DefaultExecutableCandidates -join ', ')"
    }

    if (-not (Test-Path -LiteralPath $selectedArtifactPath -PathType Leaf)) {
        throw "Windows runtime smoke artifact not found: $selectedArtifactPath"
    }

    $resolvedExecutablePath = $selectedArtifactPath
    $artifactKind = 'direct-executable'
    if ($selectedArtifactPath -match '(?i)-setup\.exe$') {
        $artifactKind = 'nsis-installer'
        $resolvedExecutablePath = Install-PackagedRuntimeSmokeArtifact -InstallerPath $selectedArtifactPath
    }

    @(
        "selected_artifact=$selectedArtifactPath",
        "artifact_kind=$artifactKind",
        "resolved_executable=$resolvedExecutablePath"
    ) | Set-Content -LiteralPath $ResolvedArtifactLog -Encoding utf8

    return $resolvedExecutablePath
}

function Copy-AppLogEvidenceIfPresent {
    $candidatePaths = @(
        (Join-Path $env:APPDATA 'com.voicewin.app\logs\voicewin.log'),
        (Join-Path $env:LOCALAPPDATA 'com.voicewin.app\logs\voicewin.log'),
        (Join-Path $env:APPDATA 'VoiceWin\logs\voicewin.log'),
        (Join-Path $env:LOCALAPPDATA 'VoiceWin\logs\voicewin.log')
    )

    $candidatePaths |
        ForEach-Object {
            $exists = Test-Path -LiteralPath $_ -PathType Leaf
            "[exists=$exists] $_"
        } | Set-Content -LiteralPath $AppLogCandidatesLog -Encoding utf8

    foreach ($candidate in $candidatePaths) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            Copy-Item -LiteralPath $candidate -Destination $AppLogEvidence -Force
            Write-Host "Copied Windows app log evidence: $candidate -> $AppLogEvidence"
            return $candidate
        }
    }

    Write-Host "No Windows app log found in known candidate paths. See $AppLogCandidatesLog"
    return $null
}

New-Item -ItemType Directory -Force -Path $SmokeDir | Out-Null
Remove-Item $StdoutLog, $StderrLog, $EvidenceLog, $TargetFile, $ResolvedArtifactLog, $InstallLayoutLog, $AppLogCandidatesLog, $AppLogEvidence -ErrorAction SilentlyContinue
[System.IO.File]::WriteAllText($TargetFile, '', [System.Text.Encoding]::UTF8)

$RuntimeExecutablePath = Resolve-SmokeExecutablePath -RequestedArtifactPath $ArtifactPath

$notepad = $null
$process = $null
$previousRuntimeSmokeEnv = $env:VOICEWIN_RUNTIME_SMOKE_TEST
$previousTranscriptEnv = $env:VOICEWIN_RUNTIME_SMOKE_TRANSCRIPT
$previousExpectEnv = $env:VOICEWIN_RUNTIME_SMOKE_EXPECT_PROCESS

try {
    $notepad = Start-Process -FilePath 'notepad.exe' -ArgumentList @($TargetFile) -PassThru
    Wait-ForWindow -Process $notepad
    Activate-ProcessWindow -ProcessId $notepad.Id -Attempts 8

    Write-Host "Launching Windows runtime smoke executable: $RuntimeExecutablePath"
    $env:VOICEWIN_RUNTIME_SMOKE_TEST = '1'
    $env:VOICEWIN_RUNTIME_SMOKE_TRANSCRIPT = $Transcript
    $env:VOICEWIN_RUNTIME_SMOKE_EXPECT_PROCESS = 'notepad.exe'

    $process = Start-Process -FilePath $RuntimeExecutablePath -PassThru -RedirectStandardOutput $StdoutLog -RedirectStandardError $StderrLog
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

    try {
        $runtimeTimer.Stop()
    }
    catch {
    }

    Copy-AppLogEvidenceIfPresent | Out-Null
    Assert-NoFailureMarker -Paths @($StdoutLog, $StderrLog)

    if ($process.ExitCode -ne 0) {
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

    if (Test-Path -LiteralPath $InstallDir) {
        Remove-Item -LiteralPath $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
