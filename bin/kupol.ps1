# KUPOL public CLI — wraps the duo-agents engine binary.
param(
    [Parameter(Position = 0)]
    [string]$Command = "help"
)

function Show-Usage {
    @"
KUPOL — security dome over your repo
https://kupol.app

Usage:
  kupol scan [path] [-f table|json|markdown] [-o file] [--fail-on low|medium|high|critical]
  kupol serve
  kupol init
  kupol mcp
  kupol info
  kupol demo
  kupol help

--fail-on: exit 1 if any finding is at least that severe (critical > high > medium > low).
If omitted, exit with the engine's status.

.kupol.yml: fail_on is the recommended CI default (pass it as --fail-on).
ignore globs in .kupol.yml are planned — the engine has no ignore CLI flag yet
(it already skips target/, node_modules/, and .git/ internally).

Engine lookup: `$KUPOL_BIN, duo-agents on PATH,
./target/release/duo-agents, ./release_build/duo-agents
"@
}

function Find-Engine {
    if ($env:KUPOL_BIN) {
        if (Test-Path -LiteralPath $env:KUPOL_BIN -PathType Leaf) {
            return $env:KUPOL_BIN
        }
        Write-Error "kupol: KUPOL_BIN is set but not a file: $($env:KUPOL_BIN)"
        exit 1
    }

    $cmd = Get-Command duo-agents -ErrorAction SilentlyContinue
    if ($cmd) {
        return $cmd.Source
    }

    $cands = @(
        ".\target\release\duo-agents",
        ".\target\release\duo-agents.exe",
        ".\release_build\duo-agents",
        ".\release_build\duo-agents.exe"
    )
    foreach ($c in $cands) {
        if (Test-Path -LiteralPath $c -PathType Leaf) {
            return (Resolve-Path $c).Path
        }
    }

    Write-Error "kupol: engine not found. Set KUPOL_BIN, install duo-agents on PATH, or build with cargo build --release."
    exit 1
}

function Get-SevRank([string]$Name) {
    switch ($Name.ToLowerInvariant()) {
        "critical" { return 4 }
        "high" { return 3 }
        "medium" { return 2 }
        "low" { return 1 }
        default { return 0 }
    }
}

function Test-JsonHitsThreshold([string]$JsonPath, [string]$Threshold) {
    $thr = Get-SevRank $Threshold
    if ($thr -lt 1) {
        Write-Error "kupol scan: --fail-on must be low|medium|high|critical"
        exit 2
    }
    $data = Get-Content -LiteralPath $JsonPath -Raw | ConvertFrom-Json
    $findings = @()
    if ($null -ne $data.findings) { $findings = @($data.findings) }
    foreach ($item in $findings) {
        if ((Get-SevRank ([string]$item.severity)) -ge $thr) {
            return $true
        }
    }
    return $false
}

$engine = Find-Engine
$rest = @()
if ($args) { $rest = $args }

switch ($Command) {
    { $_ -in @("help", "-h", "--help") } { Show-Usage; break }
    "scan" {
        $path = "."
        $format = "table"
        $output = $null
        $failOn = $null
        $i = 0
        while ($i -lt $rest.Count) {
            $a = $rest[$i]
            switch ($a) {
                { $_ -in @("-f", "--format") } {
                    $format = $rest[$i + 1]
                    $i += 2
                    continue
                }
                { $_ -in @("-o", "--output") } {
                    $output = $rest[$i + 1]
                    $i += 2
                    continue
                }
                "--path" {
                    $path = $rest[$i + 1]
                    $i += 2
                    continue
                }
                "--fail-on" {
                    $failOn = $rest[$i + 1]
                    $i += 2
                    continue
                }
                default {
                    if ($a -like "-*") {
                        Write-Error "kupol scan: unknown option: $a"
                        Show-Usage
                        exit 1
                    }
                    $path = $a
                    $i += 1
                }
            }
        }
        if ($format -eq "md") { $format = "markdown" }
        if ($format -notin @("table", "json", "markdown")) {
            Write-Error "kupol scan: invalid format: $format"
            exit 1
        }

        if (-not $failOn) {
            $argv = @("scan", "--path", $path, "--format", $format)
            if ($output) { $argv += @("--output", $output) }
            & $engine @argv
            exit $LASTEXITCODE
        }

        if ((Get-SevRank $failOn) -lt 1) {
            Write-Error "kupol scan: --fail-on must be low|medium|high|critical"
            exit 1
        }

        $jsonTmp = [System.IO.Path]::GetTempFileName()
        try {
            $jsonArgs = @("scan", "--path", $path, "--format", "json", "--output", $jsonTmp)
            & $engine @jsonArgs | Out-Null

            if ($format -eq "json") {
                if ($output) {
                    Copy-Item -LiteralPath $jsonTmp -Destination $output -Force
                } else {
                    Get-Content -LiteralPath $jsonTmp -Raw
                }
            } else {
                $disp = @("scan", "--path", $path, "--format", $format)
                if ($output) { $disp += @("--output", $output) }
                & $engine @disp
            }

            if (Test-JsonHitsThreshold $jsonTmp $failOn) {
                Write-Error "kupol: findings at or above --fail-on $failOn"
                exit 1
            }
            exit 0
        } finally {
            Remove-Item -LiteralPath $jsonTmp -ErrorAction SilentlyContinue
        }
    }
    { $_ -in @("serve", "init", "mcp", "info", "demo") } {
        & $engine $Command @rest
        exit $LASTEXITCODE
    }
    default {
        Write-Error "kupol: unknown command: $Command"
        Show-Usage
        exit 1
    }
}
