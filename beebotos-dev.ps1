#!/usr/bin/env pwsh
# BeeBotOS Development Manager (Windows)
# Usage: .\beebotos-dev.ps1 [menu|build|start|stop|restart|run|pack|status] [service|all]

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Resolve-Path $ScriptDir
$PidDir = Join-Path $ProjectRoot "data\run"
$LogDir = Join-Path $ProjectRoot "data\logs"
New-Item -ItemType Directory -Force -Path $PidDir | Out-Null
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

Set-Location $ProjectRoot

$HostIsWindows = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
    [System.Runtime.InteropServices.OSPlatform]::Windows
)

function Get-PackageRustTarget {
    if (-not [string]::IsNullOrWhiteSpace($env:BEEBOTOS_PACKAGE_TARGET)) {
        return $env:BEEBOTOS_PACKAGE_TARGET
    }
    if (-not $HostIsWindows) {
        return "x86_64-pc-windows-gnu"
    }
    return $null
}

function Get-TargetArgs($cargoTarget) {
    if ([string]::IsNullOrWhiteSpace($cargoTarget)) { return @() }
    return @("--target", $cargoTarget)
}

function Get-ReleaseDir($cargoTarget) {
    if ([string]::IsNullOrWhiteSpace($cargoTarget)) {
        return Join-Path $ProjectRoot "target\release"
    }
    return Join-Path $ProjectRoot "target\$cargoTarget\release"
}

function Get-BinaryPath($binaryName, $cargoTarget) {
    $suffix = if ($HostIsWindows -or $cargoTarget -like "*windows*") { ".exe" } else { "" }
    return Join-Path (Get-ReleaseDir $cargoTarget) "$binaryName$suffix"
}

function Invoke-CargoBuild($cargoArgs, $cargoTarget) {
    $argsWithTarget = @($cargoArgs) + (Get-TargetArgs $cargoTarget)
    & cargo @argsWithTarget
}

function Print-Header {
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host "  BeeBotOS Development Manager" -ForegroundColor Cyan
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host ""
}

function Print-Error($msg)   { Write-Host "[ERROR] $msg" -ForegroundColor Red }
function Print-Info($msg)    { Write-Host "[INFO] $msg" -ForegroundColor Blue }
function Print-Success($msg) { Write-Host "[OK] $msg" -ForegroundColor Green }
function Print-Warn($msg)    { Write-Host "[WARN] $msg" -ForegroundColor Yellow }

# Service definitions
$Services = @(
    @{
        Name = "gateway"
        Package = "beebotos-gateway"
        BuildCmd = "cargo build --release -p beebotos-gateway"
        Binary = "target\release\beebotos-gateway.exe"
        Port = 8000
        Desc = "API Gateway"
    },
    @{
        Name = "web"
        Package = "beebotos-web"
        BuildCmd = $null  # handled specially in Build-Service
        Binary = "target\release\web-server.exe"
        Port = 8090
        Desc = "Web Frontend Server"
    },
    @{
        Name = "beehub"
        Package = "beebotos-beehub"
        BuildCmd = "cargo build --release -p beebotos-beehub"
        Binary = "target\release\beehub.exe"
        Port = 8080
        Desc = "BeeHub Service"
    },
    @{
        Name = "launcher"
        Package = "beebotos-launcher"
        BuildCmd = "cargo build --release -p beebotos-launcher"
        Binary = $null
        Port = 0
        Desc = "BeeBotOS Launcher"
    },
    @{
        Name = "cli"
        BuildCmd = "cargo install --path apps\cli --force"
        Binary = $null
        Port = 0
        Desc = "CLI Tool (install only)"
    }
)

function Get-Service($name) {
    foreach ($svc in $Services) {
        if ($svc.Name -eq $name) { return $svc }
    }
    return $null
}

function Get-ServiceNames() {
    return $Services | ForEach-Object { $_.Name }
}

function Get-PidFile($name) {
    return Join-Path $PidDir "$name.pid"
}

function Test-IsRunning($name) {
    $pidFile = Get-PidFile $name
    if (Test-Path $pidFile) {
        $procId = Get-Content $pidFile -Raw
        $procId = $procId.Trim()
        try {
            $proc = Get-Process -Id $procId -ErrorAction SilentlyContinue
            if ($proc) { return $true }
        } catch {}
    }
    return $false
}

function Build-Service($name, $cargoTarget = $null) {
    $svc = Get-Service $name
    if (-not $svc) { Print-Error "Unknown service: $name"; return $false }

    Write-Host "----------------------------------------" -ForegroundColor Cyan
    Write-Host "Building: $($svc.Desc) ($name)" -ForegroundColor Cyan
    Write-Host "----------------------------------------" -ForegroundColor Cyan
    if (-not [string]::IsNullOrWhiteSpace($cargoTarget)) {
        Print-Info "Cargo target: $cargoTarget"
    }

    if (-not $svc.BuildCmd -and $name -ne "web") {
        Print-Warn "No build command for $name, skipping."
        return $true
    }

    # Check for cargo
    try {
        $null = Get-Command cargo -ErrorAction Stop
    } catch {
        Print-Error "cargo not found in PATH. Please install Rust: https://rustup.rs"
        return $false
    }

    # Special handling for web service which has multi-step build
    if ($name -eq "web") {
        try {
            $null = Get-Command trunk -ErrorAction Stop
        } catch {
            Print-Error "trunk not found in PATH. Please install it: cargo install trunk"
            return $false
        }

        # cargo build --release --lib -p beebotos-web --target wasm32-unknown-unknown
        # if ($LASTEXITCODE -ne 0) {
        #     Print-Error "Build failed: web - cargo build lib failed (exit $LASTEXITCODE)"
        #     return $false
        # }
        Push-Location (Join-Path $ProjectRoot "apps\web")
        $oldNoColor = $env:NO_COLOR
        try {
            if ($env:NO_COLOR -eq "1") {
                $env:NO_COLOR = "true"
            }
            trunk build --release
            if ($LASTEXITCODE -ne 0) {
                Print-Error "Build failed: web - trunk build failed (exit $LASTEXITCODE)"
                return $false
            }
        } finally {
            if ($null -eq $oldNoColor) {
                Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue
            } else {
                $env:NO_COLOR = $oldNoColor
            }
            Pop-Location
        }
        # wasm-pack build --target web --out-dir pkg apps/web/
        # if ($LASTEXITCODE -ne 0) {
        #     Print-Error "Build failed: web - wasm-pack build failed (exit $LASTEXITCODE)"
        #     return $false
        # }
        Invoke-CargoBuild -cargoArgs @("build", "-p", "beebotos-web", "--bin", "web-server", "--features", "server", "--release") -cargoTarget $cargoTarget
        if ($LASTEXITCODE -ne 0) {
            Print-Error "Build failed: web - cargo build web-server failed (exit $LASTEXITCODE)"
            return $false
        }
        Print-Success "Build completed: $name"
        return $true
    }

    try {
        if ($svc.Package) {
            Invoke-CargoBuild -cargoArgs @("build", "--release", "-p", $svc.Package) -cargoTarget $cargoTarget
        } else {
            Invoke-Expression $svc.BuildCmd
        }
        if ($LASTEXITCODE -eq 0 -or $null -eq $LASTEXITCODE) {
            Print-Success "Build completed: $name"
            return $true
        } else {
            Print-Error "Build failed: $name (exit $LASTEXITCODE)"
            return $false
        }
    } catch {
        Print-Error "Build failed: $name - $($_.Exception.Message)"
        return $false
    }
}

function Start-Service($name) {
    $svc = Get-Service $name
    if (-not $svc) { Print-Error "Unknown service: $name"; return $false }

    if (-not $svc.Binary) {
        Print-Warn "$name is not a daemon service, skipping start."
        return $true
    }

    $pidFile = Get-PidFile $name
    if (Test-IsRunning $name) {
        $procId = (Get-Content $pidFile -Raw).Trim()
        Print-Warn "$name is already running (PID: $procId)"
        return $true
    }

    $binaryPath = Join-Path $ProjectRoot $svc.Binary
    if (-not (Test-Path $binaryPath)) {
        Print-Error "Binary not found: $binaryPath"
        Print-Info "Please build $name first."
        return $false
    }

    Write-Host "Starting: $($svc.Desc) ($name)" -ForegroundColor Cyan
    Print-Info "Binary: $binaryPath"
    Print-Info "Port: $($svc.Port)"

    $outFile = Join-Path $LogDir "$name.log"
    $errFile = Join-Path $LogDir "$name.err"

    # web-server needs correct static-path and gateway-url to work properly
    $startArgs = @{}
    if ($name -eq "web") {
        # 准备临时静态目录，使用 trunk 生成的 apps/web/dist
        $tempStaticDir = Join-Path $ProjectRoot "data\temp-web-static"
        $distSource = Join-Path $ProjectRoot "apps\web\dist"
        if (-not (Test-Path $distSource)) {
            Print-Error "Web dist directory not found: $distSource"
            Print-Info "Please build web first: .\beebotos-dev.ps1 build web"
            return $false
        }
        if (Test-Path $tempStaticDir) { Remove-Item -Recurse -Force $tempStaticDir }
        New-Item -ItemType Directory -Force -Path $tempStaticDir | Out-Null
        Copy-Item -Recurse (Join-Path $distSource "*") $tempStaticDir
        $startArgs["ArgumentList"] = "`"--static-path`" `"$tempStaticDir`" `"--gateway-url`" http://localhost:8000"
        Print-Info "Static path: $tempStaticDir"
        Print-Info "Gateway URL: http://localhost:8000"
    }

    $proc = Start-Process -FilePath $binaryPath @startArgs -RedirectStandardOutput $outFile -RedirectStandardError $errFile -PassThru -WindowStyle Hidden
    $proc.Id | Set-Content $pidFile -NoNewline

    Start-Sleep -Seconds 1
    try {
        $check = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
        if ($check) {
            Print-Success "$name started (PID: $($proc.Id))"
            return $true
        }
    } catch {}

    Print-Error "$name failed to start. Check $outFile"
    Remove-Item $pidFile -Force -ErrorAction SilentlyContinue
    return $false
}

function Stop-Service($name) {
    $pidFile = Get-PidFile $name
    if (-not (Test-IsRunning $name)) {
        Print-Warn "$name is not running"
        Remove-Item $pidFile -Force -ErrorAction SilentlyContinue
        return
    }

    $procId = (Get-Content $pidFile -Raw).Trim()
    Write-Host "Stopping $name (PID: $procId)..." -ForegroundColor Cyan

    try {
        Stop-Process -Id $procId -Force -ErrorAction Stop
        Print-Success "$name stopped"
    } catch {
        Print-Warn "Could not stop $name gracefully: $($_.Exception.Message)"
    }
    Remove-Item $pidFile -Force -ErrorAction SilentlyContinue
}

function Restart-Service($name) {
    Stop-Service $name
    Start-Sleep -Seconds 1
    Start-Service $name | Out-Null
}

function Build-And-Start($name) {
    if (Build-Service $name) {
        Start-Service $name | Out-Null
    }
}

function Copy-RequiredFile($source, $destination) {
    if (-not (Test-Path $source)) {
        Print-Error "Required file not found: $source"
        return $false
    }
    Copy-Item $source $destination
    return $true
}

function Pack-Release($target = "all") {
    Write-Host "----------------------------------------" -ForegroundColor Cyan
    Write-Host "Packing release for target: $target" -ForegroundColor Cyan
    Write-Host "----------------------------------------" -ForegroundColor Cyan

    $cargoTarget = Get-PackageRustTarget
    $archiveTarget = if ([string]::IsNullOrWhiteSpace($cargoTarget)) { "native-windows" } else { $cargoTarget }
    $outDir = Join-Path $ProjectRoot "dist\beebotos"
    $archive = Join-Path $ProjectRoot "dist\beebotos-$archiveTarget.zip"

    if (-not [string]::IsNullOrWhiteSpace($cargoTarget)) {
        Print-Info "Packaging cargo target: $cargoTarget"
        try {
            $installedTargets = rustup target list --installed
            if ($cargoTarget -notin $installedTargets) {
                Print-Error "Rust target is not installed: $cargoTarget"
                Print-Info "Install it with: rustup target add $cargoTarget"
                exit 1
            }
        } catch {
            Print-Warn "Could not verify installed rust targets: $($_.Exception.Message)"
        }
    } else {
        Print-Info "Packaging native Windows target"
    }

    $buildList = if ($target -eq "all") { @("gateway", "web", "beehub") } elseif ($target -eq "launcher") { @() } else { @($target) }
    foreach ($svcName in $buildList) {
        if ($svcName -eq "cli") { continue }
        if (-not (Build-Service $svcName $cargoTarget)) {
            Print-Error "Cannot pack because build failed: $svcName"
            exit 1
        }
    }
    if ($target -eq "all" -or $target -eq "launcher") {
        if ($HostIsWindows) {
            if (-not (Build-Service "launcher" $cargoTarget)) {
                Print-Error "Cannot pack because build failed: launcher"
                exit 1
            }
        } elseif ($target -eq "launcher") {
            Print-Error "Launcher packaging requires native Windows."
            exit 1
        } else {
            Print-Warn "Launcher packaging is skipped outside native Windows packaging."
        }
    }

    if (Test-Path $outDir) { Remove-Item -Recurse -Force $outDir }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null

    if ($target -eq "all" -or $target -eq "gateway") {
        if (-not (Copy-RequiredFile (Get-BinaryPath "beebotos-gateway" $cargoTarget) $outDir)) { exit 1 }
        Copy-Item -Recurse (Join-Path $ProjectRoot "migrations_sqlite") $outDir
    }
    if ($target -eq "all" -or $target -eq "web") {
        if (-not (Copy-RequiredFile (Get-BinaryPath "web-server" $cargoTarget) $outDir)) { exit 1 }
        $pkgSource = Join-Path $ProjectRoot "apps\web\dist"
        $pkgDest = $outDir
        if (-not (Test-Path $pkgSource)) {
            Print-Error "Web dist directory not found: $pkgSource"
            Print-Info "Please build the web service first: .\beebotos-dev.ps1 build web"
            Remove-Item -Recurse -Force $outDir -ErrorAction SilentlyContinue
            exit 1
        }
        Get-ChildItem -Path $pkgSource | ForEach-Object {
            Copy-Item -Path $_.FullName -Destination $pkgDest -Recurse -Force
        }
    }
    if ($target -eq "all" -or $target -eq "beehub") {
        $beehubPath = Get-BinaryPath "beehub" $cargoTarget
        if (Test-Path $beehubPath) {
            Copy-Item $beehubPath $outDir
        } else {
            Print-Warn "beehub.exe not found, skipping"
        }
    }
    if ($HostIsWindows -and ($target -eq "all" -or $target -eq "launcher")) {
        if (-not (Copy-RequiredFile (Get-BinaryPath "beebotos-launcher" $cargoTarget) $outDir)) { exit 1 }
    }

    if (Test-Path (Join-Path $ProjectRoot "config")) {
        Copy-Item -Recurse (Join-Path $ProjectRoot "config") $outDir
        # 调整 web-server 生产配置：静态文件路径指向当前目录
        $prodConfig = Join-Path $outDir "config\web-server.toml"
        if (Test-Path $prodConfig) {
            (Get-Content $prodConfig) `
                -replace 'path = "apps/web/dist"', 'path = "."' `
                -replace 'path = "apps/web"', 'path = "."' |
                Set-Content $prodConfig -Encoding UTF8
        }
    }

    if (Test-Path (Join-Path $ProjectRoot "skills")) {
        Copy-Item -Recurse (Join-Path $ProjectRoot "skills") $outDir
    }

    if (Test-Path (Join-Path $ProjectRoot "workflows")) {
        Copy-Item -Recurse (Join-Path $ProjectRoot "workflows") $outDir
    }

    Copy-Item (Join-Path $ProjectRoot "beebotos-run.ps1") $outDir

    Compress-Archive -Path $outDir -DestinationPath $archive -Force
    Print-Success "Release packed: $archive"
    Write-Host "Contents:"
    Get-ChildItem $outDir | Format-Table Name, @{Label="Size"; Expression={$_.Length}; Align="Right"}
}

function Show-Status {
    Write-Host "Service Status" -ForegroundColor Cyan
    Write-Host "----------------------------------------" -ForegroundColor Cyan
    Write-Host ("{0,-12} {1,-10} {2,-8} {3}" -f "Service", "Status", "PID", "Port")
    Write-Host "----------------------------------------"
    foreach ($svc in $Services) {
        if (-not $svc.Binary) {
            Write-Host ("{0,-12} {1,-10} {2,-8} {3}" -f $svc.Name, "N/A", "-", "install-only")
            continue
        }
        $pidFile = Get-PidFile $svc.Name
        if (Test-IsRunning $svc.Name) {
            $procId = (Get-Content $pidFile -Raw).Trim()
            $line = "{0,-12} {1,-10} {2,-8} {3}" -f $svc.Name, "running", $procId, $svc.Port
            Write-Host $line -ForegroundColor Green
        } else {
            $line = "{0,-12} {1,-10} {2,-8} {3}" -f $svc.Name, "stopped", "-", $svc.Port
            Write-Host $line -ForegroundColor Red
        }
    }
}

function Show-Menu {
    Clear-Host
    Print-Header
    Write-Host "  1) Build"
    Write-Host "     1.1) Build Gateway"
    Write-Host "     1.2) Build Web"
    Write-Host "     1.3) Build CLI"
    Write-Host "     1.4) Build BeeHub"
    Write-Host "     1.5) Build All"
    Write-Host ""
    Write-Host "  2) Start"
    Write-Host "     2.1) Start Gateway"
    Write-Host "     2.2) Start Web"
    Write-Host "     2.3) Start BeeHub"
    Write-Host "     2.4) Start All"
    Write-Host ""
    Write-Host "  3) Stop"
    Write-Host "     3.1) Stop Gateway"
    Write-Host "     3.2) Stop Web"
    Write-Host "     3.3) Stop BeeHub"
    Write-Host "     3.4) Stop All"
    Write-Host ""
    Write-Host "  4) Restart"
    Write-Host "     4.1) Restart Gateway"
    Write-Host "     4.2) Restart Web"
    Write-Host "     4.3) Restart BeeHub"
    Write-Host "     4.4) Restart All"
    Write-Host ""
    Write-Host "  5) Build & Start"
    Write-Host "     5.1) Build & Start Gateway"
    Write-Host "     5.2) Build & Start Web"
    Write-Host "     5.3) Build & Start BeeHub"
    Write-Host "     5.4) Build & Start All"
    Write-Host ""
    Write-Host "  6) Status"
    Write-Host "  7) Pack Release"
    Write-Host "  0) Exit"
    Write-Host ""
    $choice = Read-Host "Select option"
    return $choice
}

function Handle-Menu {
    while ($true) {
        $choice = Show-Menu
        Write-Host ""

        switch ($choice) {
            { $_ -in "1", "1.1" } { Build-Service "gateway" }
            "1.2" { Build-Service "web" }
            "1.3" { Build-Service "cli" }
            "1.4" { Build-Service "beehub" }
            "1.5" {
                foreach ($svc in @("gateway", "web", "cli", "beehub", "launcher")) {
                    Build-Service $svc | Out-Null
                }
            }
            { $_ -in "2", "2.1" } { Start-Service "gateway" | Out-Null }
            "2.2" { Start-Service "web" | Out-Null }
            "2.3" { Start-Service "beehub" | Out-Null }
            "2.4" {
                foreach ($svc in @("gateway", "web", "beehub")) {
                    Start-Service $svc | Out-Null
                }
            }
            { $_ -in "3", "3.1" } { Stop-Service "gateway" }
            "3.2" { Stop-Service "web" }
            "3.3" { Stop-Service "beehub" }
            "3.4" {
                foreach ($svc in @("gateway", "web", "beehub")) {
                    Stop-Service $svc
                }
            }
            { $_ -in "4", "4.1" } { Restart-Service "gateway" }
            "4.2" { Restart-Service "web" }
            "4.3" { Restart-Service "beehub" }
            "4.4" {
                foreach ($svc in @("gateway", "web", "beehub")) {
                    Restart-Service $svc
                }
            }
            { $_ -in "5", "5.1" } { Build-And-Start "gateway" }
            "5.2" { Build-And-Start "web" }
            "5.3" { Build-And-Start "beehub" }
            "5.4" {
                foreach ($svc in @("gateway", "web", "beehub")) {
                    Build-And-Start $svc
                }
            }
            "6" { Show-Status }
            "7" { Pack-Release "all" }
            { $_ -in "0", "q", "quit", "exit" } { Write-Host "Goodbye!"; exit 0 }
            default { Print-Warn "Invalid option: $choice" }
        }

        Write-Host ""
        Read-Host "Press Enter to continue"
    }
}

function Handle-Cli($action, $target = "all") {
    $validServices = Get-ServiceNames
    if ($target -ne "all" -and $target -notin $validServices) {
        Print-Error "Unknown service: $target"
        Print-Info "Available: $($validServices -join ' ') all"
        exit 1
    }

    switch ($action) {
        "build" {
            $list = if ($target -eq "all") { @("gateway", "web", "cli", "beehub", "launcher") } else { @($target) }
            foreach ($svc in $list) { Build-Service $svc | Out-Null }
        }
        "start" {
            $list = if ($target -eq "all") { @("gateway", "web", "beehub") } else { @($target) }
            foreach ($svc in $list) { Start-Service $svc | Out-Null }
        }
        "stop" {
            $list = if ($target -eq "all") { @("gateway", "web", "beehub") } else { @($target) }
            foreach ($svc in $list) { Stop-Service $svc }
        }
        "restart" {
            $list = if ($target -eq "all") { @("gateway", "web", "beehub") } else { @($target) }
            foreach ($svc in $list) { Restart-Service $svc }
        }
        "run" {
            $list = if ($target -eq "all") { @("gateway", "web", "beehub") } else { @($target) }
            foreach ($svc in $list) { Build-And-Start $svc }
        }
        "pack" { Pack-Release $target }
        "status" { Show-Status }
        default {
            Print-Error "Unknown action: $action"
            Write-Host "Usage: beebotos-dev.ps1 [menu|build|start|stop|restart|run|pack|status] [service|all]"
            Write-Host ""
            Write-Host "Actions:"
            Write-Host "  build    - Compile a service"
            Write-Host "  start    - Start a service"
            Write-Host "  stop     - Stop a service"
            Write-Host "  restart  - Restart a service"
            Write-Host "  run      - Build and start a service"
            Write-Host "  pack     - Package binaries and assets for deployment"
            Write-Host "  status   - Show service status"
            Write-Host "  menu     - Interactive menu (default)"
            Write-Host ""
            Write-Host "Services: $($validServices -join ' ') all"
            exit 1
        }
    }
}

$action = if ($args.Count -gt 0) { $args[0] } else { "menu" }

if ($action -eq "menu") {
    Handle-Menu
} else {
    $target = if ($args.Count -gt 1) { $args[1] } else { "all" }
    Handle-Cli $action $target
}
