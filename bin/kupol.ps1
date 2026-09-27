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
  kupol scan [path ...] [-f table|json|markdown] [-o file] [--fail-on low|medium|high|critical]
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
    $c = $h = $m = $l = $inf = 0
    foreach ($item in $findings) {
        $sev = ([string]$item.severity).ToLowerInvariant()
        switch ($sev) {
            "critical" { $c++ }
            "high" { $h++ }
            "medium" { $m++ }
            "low" { $l++ }
            default { $inf++ }
        }
    }
    Write-Error "kupol: count critical=$c high=$h medium=$m low=$l info=$inf"
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
        $paths = [System.Collections.Generic.List[string]]::new()
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
                    $paths.Add($rest[$i + 1])
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
                    $paths.Add($a)
                    $i += 1
                }
            }
        }
        if ($paths.Count -eq 0) { $paths.Add(".") }
        if ($format -eq "md") { $format = "markdown" }
        if ($format -notin @("table", "json", "markdown")) {
            Write-Error "kupol scan: invalid format: $format"
            exit 1
        }

        if (-not $failOn) {
            $last = 0
            foreach ($p in $paths) {
                $argv = @("scan", "--path", $p, "--format", $format)
                if ($output -and $paths.Count -eq 1) { $argv += @("--output", $output) }
                & $engine @argv
                $last = $LASTEXITCODE
            }
            exit $last
        }

        if ((Get-SevRank $failOn) -lt 1) {
            Write-Error "kupol scan: --fail-on must be low|medium|high|critical"
            exit 1
        }

        $jsonParts = @()
        $mdParts = @()
        try {
            $merged = @{ findings = @() }
            foreach ($p in $paths) {
                $part = [System.IO.Path]::GetTempFileName()
                $jsonParts += $part
                & $engine @("scan", "--path", $p, "--format", "json", "--output", $part) | Out-Null
                $chunk = Get-Content -LiteralPath $part -Raw | ConvertFrom-Json
                if ($null -ne $chunk.findings) { $merged.findings += @($chunk.findings) }
                if ($format -ne "json") {
                    if ($output) {
                        $md = [System.IO.Path]::GetTempFileName()
                        $mdParts += $md
                        & $engine @("scan", "--path", $p, "--format", $format, "--output", $md) | Out-Null
                    } else {
                        & $engine @("scan", "--path", $p, "--format", $format)
                    }
                }
            }
            $jsonTmp = [System.IO.Path]::GetTempFileName()
            $jsonParts += $jsonTmp
            ($merged | ConvertTo-Json -Depth 20) | Set-Content -LiteralPath $jsonTmp -Encoding utf8

            if ($format -eq "json") {
                if ($output) {
                    Copy-Item -LiteralPath $jsonTmp -Destination $output -Force
                } else {
                    Get-Content -LiteralPath $jsonTmp -Raw
                }
            } elseif ($output) {
                $acc = New-Object System.Text.StringBuilder
                for ($n = 0; $n -lt $paths.Count; $n++) {
                    [void]$acc.AppendLine("# Scan: $($paths[$n])")
                    [void]$acc.AppendLine()
                    [void]$acc.AppendLine((Get-Content -LiteralPath $mdParts[$n] -Raw))
                }
                Set-Content -LiteralPath $output -Value $acc.ToString() -Encoding utf8
            }

            if (Test-JsonHitsThreshold $jsonTmp $failOn) {
                Write-Error "kupol: findings at or above --fail-on $failOn"
                exit 1
            }
            exit 0
        } finally {
            foreach ($f in ($jsonParts + $mdParts)) {
                if ($f) { Remove-Item -LiteralPath $f -ErrorAction SilentlyContinue }
            }
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
