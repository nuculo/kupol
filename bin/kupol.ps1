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
  kupol scan [path] [-f table|json|markdown] [-o file]
  kupol serve
  kupol init
  kupol mcp
  kupol info
  kupol demo
  kupol help

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

$engine = Find-Engine
$rest = @()
if ($args) { $rest = $args }

switch ($Command) {
    { $_ -in @("help", "-h", "--help") } { Show-Usage; break }
    "scan" {
        $path = "."
        $format = "table"
        $output = $null
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
        $argv = @("scan", "--path", $path, "--format", $format)
        if ($output) { $argv += @("--output", $output) }
        & $engine @argv
        exit $LASTEXITCODE
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
