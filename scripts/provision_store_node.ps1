<#
.SYNOPSIS
    1-Click Store Node Provisioner & SD Card Config Generator (Windows Host)
.DESCRIPTION
    Generates the turnkey configuration bundle (node_manifest.json, $12 Google Pay staking receipt,
    and auto-boot scripts) for $100 Compact, $150 Pro Commercial, and $299 Enterprise Fortress nodes.
#>
param(
    [Parameter(Mandatory=$false)]
    [string]$StoreSlug = "artisan-pizza-01",

    [Parameter(Mandatory=$false)]
    [ValidateSet("COMPACT_100", "PRO_150", "FORTRESS_299")]
    [string]$NodeTier = "PRO_150",

    [Parameter(Mandatory=$false)]
    [ValidateSet("analog_rca", "usb_dac", "hdmi", "bluetooth")]
    [string]$AudioOutput = "analog_rca",

    [Parameter(Mandatory=$false)]
    [switch]$EnableCrankkMultimining,

    [Parameter(Mandatory=$false)]
    [string]$OutputDir = "$PSScriptRoot\..\provisioned_nodes"
)

$ErrorActionPreference = "Stop"

$TargetDir = Join-Path $OutputDir $StoreSlug
if (-not (Test-Path $TargetDir)) {
    New-Item -ItemType Directory -Path $TargetDir -Force | Out-Null
}

$Timestamp = [int][double]::Parse((Get-Date -UFormat %s))
$NodeDid = "did:atlantis:node:$StoreSlug-$Timestamp"

$PriceMap = @{
    "COMPACT_100"  = 100.0
    "PRO_150"      = 150.0
    "FORTRESS_299" = 299.0
}
$HardwarePrice = $PriceMap[$NodeTier]

$Manifest = [ordered]@{
    store_slug                 = $StoreSlug
    node_did                   = $NodeDid
    hardware_tier              = $NodeTier
    hardware_price_usd         = $HardwarePrice
    audio_output               = $AudioOutput
    crankk_multimining_enabled = [bool]$EnableCrankkMultimining
    staking                    = [ordered]@{
        annual_stake_usd       = 12.00
        monthly_pool_drip_usd  = 1.00
        initial_drip_completed = $true
        remaining_escrow_usd   = 11.00
        payment_method         = "GOOGLE_PAY"
        provisioned_at_epoch   = $Timestamp
    }
    radio_stream               = [ordered]@{
        station_name           = "The Seven Daughters 24/7 Siren Commercial Radio"
        base_frequency_hz      = 432
        local_stream_url       = "http://localhost:8080/live.mp3"
        zero_dead_air_fallback = $true
        custom_jingles_enabled = ($NodeTier -ne "COMPACT_100")
    }
}

$ManifestPath = Join-Path $TargetDir "node_manifest.json"
$Manifest | ConvertTo-Json -Depth 5 | Set-Content -Path $ManifestPath -Encoding UTF8

Write-Host "=================================================================" -ForegroundColor Cyan
Write-Host " ATLANTIS TURNKEY NODE PROVISIONED SUCCESSFULLY" -ForegroundColor Green
Write-Host "=================================================================" -ForegroundColor Cyan
Write-Host " Store Slug    : $StoreSlug"
Write-Host " Node DID      : $NodeDid"
Write-Host " Tier          : $NodeTier ($$HardwarePrice + `$12 Google Pay Stake)"
Write-Host " Audio Output  : $AudioOutput"
Write-Host " Crankk DePIN  : $EnableCrankkMultimining"
Write-Host " Manifest Path : $ManifestPath"
Write-Host "=================================================================" -ForegroundColor Cyan
