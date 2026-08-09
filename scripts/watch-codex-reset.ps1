#Requires -Version 7.4
#Requires -PSEdition Core

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$AuthPath,

    [string]$ProviderCliPath,

    [ValidateRange(1, 86400)]
    [int]$IntervalSeconds = 300
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ProviderCliPath)) {
    $ProviderCliPath = Join-Path $PSScriptRoot "..\target\reset-monitor\provider.exe"
}
if (-not (Test-Path -LiteralPath $AuthPath -PathType Leaf)) {
    throw "Auth file does not exist: $AuthPath"
}
if (-not (Test-Path -LiteralPath $ProviderCliPath -PathType Leaf)) {
    throw "Provider CLI does not exist: $ProviderCliPath"
}

$sample = 0
while ($true) {
    $sample += 1
    $timestamp = [DateTimeOffset]::UtcNow.ToString("o")

    try {
        $errorMessage = "provider CLI request failed"
        $rawOutput = & $ProviderCliPath codex usage $AuthPath 2>$null
        $exitCode = $LASTEXITCODE
        $output = ($rawOutput | Out-String).Trim()
        if ($exitCode -ne 0) {
            throw $errorMessage
        }

        $errorMessage = "provider CLI returned invalid JSON"
        $response = ConvertFrom-Json -InputObject $output
        $rawUsedPercent = $response.rate_limit.primary_window.used_percent
        $errorMessage = "response has no numeric rate_limit.primary_window.used_percent"
        if ($null -eq $rawUsedPercent -or $rawUsedPercent -isnot [ValueType] -or
            $rawUsedPercent -is [bool]) {
            throw $errorMessage
        }
        $usedPercent = [decimal]$rawUsedPercent

        if ($usedPercent -eq 0) {
            Write-Output "CODEX_RATE|state=RESET|sample=$sample|at=$timestamp|used_percent=0"
            exit 0
        }

        Write-Output "CODEX_RATE|state=WAIT|sample=$sample|at=$timestamp|used_percent=$usedPercent|next_seconds=$IntervalSeconds"
    }
    catch {
        Write-Output "CODEX_RATE|state=ERROR|sample=$sample|at=$timestamp|message=$errorMessage|next_seconds=$IntervalSeconds"
    }

    Start-Sleep -Seconds $IntervalSeconds
}
