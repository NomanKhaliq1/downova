# Installs, upgrades and removes the installer in a throw-away folder and checks that nothing is left behind.
#   powershell -File scripts\test-installer.ps1
# It touches the current user's registry for a few seconds (Uninstall entry, browser helper keys, autostart value,
# Start Menu shortcut) and always removes it again. The app itself runs against temporary data folders.
$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent $PSScriptRoot
$brand = Get-Content "$root\brand\brand.toml" -Raw
$product = [regex]::Match($brand, '(?m)^product_id\s*=\s*"([^"]+)"').Groups[1].Value
$name = [regex]::Match($brand, '(?m)^name\s*=\s*"([^"]+)"').Groups[1].Value
$guid = [regex]::Match($brand, '(?m)^app_guid\s*=\s*"([^"]+)"').Groups[1].Value
$version = [regex]::Match((Get-Content "$root\host\Cargo.toml" -Raw), '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
$setup = "$root\dist\$product-setup-$version.exe"
if (-not (Test-Path $setup)) { throw "build it first: scripts\build-installer.ps1 ($setup)" }

$results = @()
function Check($what, $ok, $detail = '') { $script:results += [pscustomobject]@{ ok = [bool]$ok }; "{0}  {1}{2}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $what, $(if ($detail) { "  -- $detail" } else { '' }) }

$tmp = Join-Path $env:TEMP ("dm-inst-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
$inst = "$tmp\app"
New-Item -ItemType Directory "$tmp\data", "$tmp\dl" | Out-Null
$env:DM_DATA_DIR = "$tmp\data"; $env:DM_DOWNLOAD_DIR = "$tmp\dl"   # the app and the installer's helper calls use these
$uninstKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{$guid}_is1"
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$nmKeys = 'Google\Chrome', 'Microsoft\Edge', 'BraveSoftware\Brave-Browser' | ForEach-Object { "HKCU:\Software\$_\NativeMessagingHosts\com.$product.host" }
$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) "$name.lnk"
$realData = Join-Path $env:APPDATA $product

function State { [pscustomobject]@{
  uninst = Test-Path $uninstKey; run = [bool](Get-ItemProperty $runKey -Name $product -ErrorAction SilentlyContinue); nm = @($nmKeys | Where-Object { Test-Path $_ }).Count
  lnk = Test-Path $lnk; dir = Test-Path $inst } }
function Ping { try { $a = Get-Content "$tmp\data\api.json" -Raw -ErrorAction Stop | ConvertFrom-Json; Invoke-RestMethod "http://127.0.0.1:$($a.port)/api/ping" -TimeoutSec 3 } catch { $null } }
function Wait-Until($cond, $sec = 15) { $t = Get-Date; while (((Get-Date) - $t).TotalSeconds -lt $sec) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 300 }; return $false }

try {
  $s = State
  Check 'clean machine before the test (no entries, no real data folder)' (-not ($s.uninst -or $s.run -or $s.nm -or $s.lnk)) ($s | ConvertTo-Json -Compress)
  Check "no real $realData folder yet" (-not (Test-Path $realData))

  # ---- install ----
  $p = Start-Process $setup -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=$inst", '/TASKS=autostart', "/LOG=$tmp\install.log" -Wait -PassThru
  Check 'install finished with exit code 0' ($p.ExitCode -eq 0) "exit $($p.ExitCode)"
  foreach ($f in 'dmd.exe', 'dmhost.exe', 'ui\index.html', 'ui\app.js', 'extension\manifest.json', 'extension\popup.html', "com.$product.host.json", 'unins000.exe') { Check "file installed: $f" (Test-Path "$inst\$f") }
  $s = State
  Check 'Uninstall entry registered' $s.uninst
  Check 'browser helper registered for Chrome, Edge and Brave' ($s.nm -eq 3) "$($s.nm) of 3"
  Check 'start-with-Windows value written' $s.run
  Check 'Start Menu shortcut created' $s.lnk
  $runVal = (Get-ItemProperty $runKey -Name $product -ErrorAction SilentlyContinue).$product
  Check 'autostart value points into the install folder, quiet mode' ($runVal -like "*$inst*dmd.exe*--background*") $runVal
  $hostJson = Get-Content "$inst\com.$product.host.json" -Raw | ConvertFrom-Json
  Check 'helper manifest points to the installed dmhost.exe and the pinned extension ID' (($hostJson.path -eq "$inst\dmhost.exe") -and ($hostJson.allowed_origins.Count -ge 1)) $hostJson.path
  $vi = (Get-Item "$inst\dmd.exe").VersionInfo
  Check 'exe carries the product name and version' (($vi.ProductName -eq $name) -and ($vi.FileVersion -like "$version*")) "$($vi.ProductName) $($vi.FileVersion)"

  # ---- the installed app really runs ----
  Start-Process "$inst\dmd.exe" -ArgumentList '--background' | Out-Null
  $up = Wait-Until { (Ping) -ne $null }
  $pg = Ping
  Check 'installed app starts and answers' ($up -and $pg.app -eq $product) ($pg | ConvertTo-Json -Compress)
  if ($up) {
    $a = Get-Content "$tmp\data\api.json" | ConvertFrom-Json
    $ui = try { (Invoke-WebRequest "http://127.0.0.1:$($a.port)/" -UseBasicParsing -TimeoutSec 5).StatusCode } catch { 0 }
    Check 'it serves the installed UI' ($ui -eq 200)
  }

  # ---- upgrade while the app is running ----
  $before = (Get-Item "$inst\dmd.exe").LastWriteTime
  $p = Start-Process $setup -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=$inst", '/TASKS=autostart', "/LOG=$tmp\upgrade.log" -Wait -PassThru
  Check 'upgrade over a running app finished with exit code 0' ($p.ExitCode -eq 0) "exit $($p.ExitCode)"
  Check 'the running app was closed gracefully (api.json removed)' (-not (Test-Path "$tmp\data\api.json"))
  Check 'files still in place after the upgrade' ((Test-Path "$inst\dmd.exe") -and (Test-Path "$inst\ui\index.html"))
  Check 'no duplicate Uninstall entry' ((Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' | Where-Object { $_.PSChildName -like "{$guid}*" } | Measure-Object).Count -eq 1)

  # ---- uninstall (app running again) ----
  Start-Process "$inst\dmd.exe" -ArgumentList '--background' | Out-Null
  Check 'app running before uninstall' (Wait-Until { (Ping) -ne $null })
  $p = Start-Process "$inst\unins000.exe" -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait -PassThru
  Check 'uninstall finished with exit code 0' ($p.ExitCode -eq 0) "exit $($p.ExitCode)"
  Start-Sleep 2
  $s = State
  Check 'Uninstall entry removed' (-not $s.uninst)
  Check 'browser helper keys removed' ($s.nm -eq 0) "$($s.nm) left"
  Check 'start-with-Windows value removed' (-not $s.run)
  Check 'Start Menu shortcut removed' (-not $s.lnk)
  Check 'app was closed by the uninstaller' (-not (Test-Path "$tmp\data\api.json") -and ((Get-Process dmd -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$inst*" } | Measure-Object).Count -eq 0))
  $left = if (Test-Path $inst) { @(Get-ChildItem $inst -Recurse -Force | Select-Object -ExpandProperty FullName) } else { @() }
  Check 'install folder is gone' ($left.Count -eq 0) ($left -join '; ')
  Check 'user data (list, settings) kept after a silent uninstall' (Test-Path "$tmp\data\state.json")
  Check "no real $realData folder was created" (-not (Test-Path $realData))
}
finally {
  # whatever happened above: leave nothing behind
  if (Test-Path "$inst\unins000.exe") { Start-Process "$inst\unins000.exe" -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES' -Wait | Out-Null }
  Get-Process dmd, dmhost -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$tmp*" } | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep 1
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item Env:DM_DATA_DIR, Env:DM_DOWNLOAD_DIR -ErrorAction SilentlyContinue
  $s = State
  Check 'registry and shortcut clean after the test' (-not ($s.uninst -or $s.run -or $s.nm -or $s.lnk)) ($s | ConvertTo-Json -Compress)
  Check 'temp folder removed' (-not (Test-Path $tmp))
  $bad = @($results | Where-Object { -not $_.ok }).Count
  if ($bad) { "`n$bad check(s) failed"; exit 1 } else { "`nall checks passed" }
}
