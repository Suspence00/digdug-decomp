# Differential test: scripted input on MAME (reference) vs our emulator, comparing every
# CPU access to the Namco 06XX/51XX window. Requires MAME extracted to mame_ref\mame
# (with 51xx.bin/53xx.bin in roms\digdug.zip) and ROMs in roms\extracted.
#   usage: pwsh tools\mame\run_trace51.ps1
$root = Resolve-Path "$PSScriptRoot\..\.."
$work = "$root\tools\mame"
New-Item -ItemType Directory -Force "$work\out" | Out-Null
Push-Location $root
cargo build --release --bin trace51 | Out-Null
& "$root\target\release\trace51.exe" "$work\inputscript.txt" | Set-Content "$work\out\trace51_ours.txt"
Pop-Location
$p = Start-Process -FilePath "$root\mame_ref\mame\mame.exe" -WorkingDirectory $work -PassThru -WindowStyle Hidden `
  -ArgumentList "digdug","-video","none","-sound","none","-nothrottle","-skip_gameinfo","-seconds_to_run","60",
                "-autoboot_script","$work\trace51.lua","-rompath","$root\mame_ref\mame\roms"
if (-not $p.WaitForExit(120000)) { $p.Kill(); throw "MAME timed out" }
"ours: $((Get-Content "$work\out\trace51_ours.txt").Count) events   mame: $((Get-Content "$work\out\trace51_mame.txt").Count) events"
