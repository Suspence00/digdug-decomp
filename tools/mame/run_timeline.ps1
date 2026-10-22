# Compare native and MAME timelines from power-on. Requires user-supplied Rev 2
# program ROMs in roms/extracted and a MAME ROM set including 51xx/53xx firmware.
# Exit: 0 matched, 1 diverged, 2 inconclusive, 64 setup/capture error.
[CmdletBinding()]
param(
    [string]$Script = "$PSScriptRoot\inputscript.txt",
    $Frames = 1600,
    $ScriptStart = 700,
    [string]$MamePath = "$PSScriptRoot\..\..\mame_ref\mame\mame.exe",
    [string]$MameRomPath = "$PSScriptRoot\..\..\mame_ref\mame\roms",
    $TimeoutSeconds = 120
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path -LiteralPath "$PSScriptRoot\..\..").Path

try {
    $frameCount = 0
    $firstScriptFrame = 0
    $timeoutLimit = 0
    if (-not [int]::TryParse([string]$Frames, [ref]$frameCount) -or
        -not [int]::TryParse([string]$ScriptStart, [ref]$firstScriptFrame) -or
        $firstScriptFrame -lt 1 -or $frameCount -lt $firstScriptFrame -or $frameCount -gt 10000) {
        throw 'Require 1 <= ScriptStart <= Frames <= 10000'
    }
    if (-not [int]::TryParse([string]$TimeoutSeconds, [ref]$timeoutLimit) -or
        $timeoutLimit -lt 1 -or $timeoutLimit -gt 3600) {
        throw 'TimeoutSeconds must be an integer from 1 to 3600'
    }
    $Frames = $frameCount
    $ScriptStart = $firstScriptFrame
    $TimeoutSeconds = $timeoutLimit
    $scriptPath = (Resolve-Path -LiteralPath $Script).Path
    $mameExe = (Resolve-Path -LiteralPath $MamePath).Path
    $romPath = (Resolve-Path -LiteralPath $MameRomPath).Path
    $runId = (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0, 8)
    $runDir = Join-Path $PSScriptRoot "out\timeline\$runId"
    New-Item -ItemType Directory -Path (Join-Path $runDir 'nvram\digdug') -Force | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $runDir 'nvram\digdug\earom'), [byte[]]::new(64))
    Copy-Item -LiteralPath $scriptPath -Destination (Join-Path $runDir 'inputscript.txt')
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'timeline.lua') -Destination (Join-Path $runDir 'timeline.lua')
    "return { frames = $Frames, script_start = $ScriptStart }" |
        Set-Content -LiteralPath (Join-Path $runDir 'timeline_options.lua') -Encoding ascii
    (& $mameExe -help | Select-Object -First 1) |
        Set-Content -LiteralPath (Join-Path $runDir 'mame.version.txt') -Encoding utf8

    Push-Location -LiteralPath $root
    try {
        & cargo build --locked --release --bin timeline
        if ($LASTEXITCODE -ne 0) { throw "Native build failed ($LASTEXITCODE)" }
        $timelineExe = Join-Path $root 'target\release\timeline.exe'
        & $timelineExe capture --script (Join-Path $runDir 'inputscript.txt') --script-start $ScriptStart `
            --frames $Frames --output (Join-Path $runDir 'native.timeline')
        if ($LASTEXITCODE -ne 0) { throw "Native capture failed ($LASTEXITCODE)" }
    }
    finally { Pop-Location }

    # MAME must not load user INIs, plugins, saved configurations or NVRAM. The
    # Lua checks time zero, pins DIPs, and verifies the physical input cache for
    # each CPU frame. Nonzero frame-one inputs are explicitly inconclusive.
    # Start-Process joins ArgumentList into a command line, so quote each argument
    # to preserve spaces in user-supplied executable/ROM paths.
    $mameArgs = @(
        'digdug', '-video', 'none', '-sound', 'none', '-nothrottle', '-skip_gameinfo',
        '-noreadconfig', '-noplugins', '-nonvram_save', '-cfg_directory', 'cfg',
        '-nvram_directory', 'nvram', '-autoboot_delay', '0', '-autoboot_script', 'timeline.lua',
        '-rompath', $romPath, '-seconds_to_run', ([Math]::Ceiling($Frames / 60.0) + 5).ToString()
    )
    $quotedArgs = $mameArgs | ForEach-Object { '"' + $_.Replace('"', '\"') + '"' }
    $process = Start-Process -FilePath $mameExe -WorkingDirectory $runDir -PassThru -WindowStyle Hidden `
        -ArgumentList $quotedArgs -RedirectStandardOutput (Join-Path $runDir 'mame.stdout.txt') `
        -RedirectStandardError (Join-Path $runDir 'mame.stderr.txt')
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while (-not $process.WaitForExit(1000)) {
        if ([DateTime]::UtcNow -ge $deadline) {
            $process.Kill()
            $process.WaitForExit()
            throw "MAME capture timed out; inspect $runDir"
        }
    }
    if ($process.ExitCode -ne 0) { throw "MAME exited $($process.ExitCode); inspect $runDir" }
    if (Test-Path -LiteralPath (Join-Path $runDir 'timeline.error.txt')) {
        throw (Get-Content -LiteralPath (Join-Path $runDir 'timeline.error.txt') -Raw)
    }
    if (Test-Path -LiteralPath (Join-Path $runDir 'timeline.inconclusive.txt')) {
        $reason = Get-Content -LiteralPath (Join-Path $runDir 'timeline.inconclusive.txt') -Raw
        "Dig Dug frame timeline: INCONCLUSIVE`n$reason" |
            Tee-Object -FilePath (Join-Path $runDir 'report.txt')
        Write-Host "Timeline artifacts: $runDir"
        exit 2
    }
    if (-not (Test-Path -LiteralPath (Join-Path $runDir 'timeline.complete'))) {
        throw "MAME did not complete the capture; inspect $runDir"
    }
    $completedFrames = (Get-Content -LiteralPath (Join-Path $runDir 'timeline.complete') -Raw).Trim()
    if ($completedFrames -ne $Frames.ToString()) { throw "MAME captured $completedFrames frames; expected $Frames" }

    & $timelineExe compare --native (Join-Path $runDir 'native.timeline') `
        --reference (Join-Path $runDir 'reference.timeline') |
        Tee-Object -FilePath (Join-Path $runDir 'report.txt')
    $comparisonExit = $LASTEXITCODE
    Write-Host "Timeline artifacts: $runDir"
    exit $comparisonExit
}
catch {
    Write-Error -Message $_ -ErrorAction Continue
    exit 64
}
