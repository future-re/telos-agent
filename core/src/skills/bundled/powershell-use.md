---
name: powershell-use
description: Use when writing PowerShell commands, scripts, or functions. Provides PowerShell 5.1 patterns: script structure, naming, parameters, pipeline, error handling, and code style.
whenToUse: When the user asks to write, debug, or review PowerShell commands, scripts, or functions.
prompt: |
  You are writing PowerShell 5.1 commands. Prefer PowerShell 5.1 syntax. Follow these rules:

  ## Structure
  - `#Requires -Version 5.1` at the top of scripts.
  - `[CmdletBinding()]` on every function. `SupportsShouldProcess` for destructive ops.
  - `begin {}`, `process {}`, `end {}` blocks.
  - Comment-based help: `.SYNOPSIS`, `.DESCRIPTION`, `.PARAMETER`, `.EXAMPLE`.

  ## Naming and Parameters
  - Verb-Noun with approved verbs (`Get-Verb`). PascalCase. Singular nouns.
  - `[Parameter(Mandatory)]`, `[ValidateNotNullOrEmpty()]`, `[ValidateSet()]`, `[ValidateRange()]`.
  - `ValueFromPipeline` / `ValueFromPipelineByPropertyName` for pipeline input.
  - Standard optional parameters: `-Force`, `-PassThru`, `-WhatIf`, `-Confirm`.

  ## Pipeline
  - Stream output immediately with `Write-Output` in `process {}`. Do NOT buffer into arrays.
  - Return typed objects: `[PSCustomObject]@{ PSTypeName = 'Module.Name'; Prop = $val }`.

  ## Error Handling
  - `try { ... } catch [System.IO.FileNotFoundException] { ... } catch { throw }`
  - `-ErrorAction Stop` to make non-terminating errors catchable.
  - `Write-Error`, `Write-Warning`, `Write-Verbose`, `Write-Debug`, `Write-Progress`.

  ## Code Style
  - NO aliases: `Get-ChildItem` not `gci`; `Where-Object` not `?`; `ForEach-Object` not `%`.
  - Explicit parameter names over positional binding.
  - Splatting: `$p = @{ Path = $s; Dest = $d; Force = $true }; Copy-Item @p`.
  - Line continuations via `|` after pipe operators, never backticks.

  ## Modules
  - PS 5.1: `Find-Module`, `Install-Module -Scope CurrentUser -Force`.
  - PS 7.4+: `Find-PSResource`, `Install-PSResource -Scope CurrentUser -TrustRepository`.
  - Check: `Get-Module -Name Microsoft.PowerShell.PSResourceGet -ListAvailable` before using modern cmdlets.
  - Common: `Pester`, `PSScriptAnalyzer` (testing); `PSReadLine`, `Terminal-Icons` (console); `Az.*` (Azure); `ImportExcel` (Excel).
---

# PowerShell 5.1 Quick Reference

## Script Template
```powershell
#Requires -Version 5.1
<#
.SYNOPSIS
    Brief description.
.DESCRIPTION
    Detailed description.
.PARAMETER Name
    Parameter description.
.EXAMPLE
    Example-Usage -Name 'Value'
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory, ValueFromPipeline)]
    [ValidateNotNullOrEmpty()]
    [string[]]$Name,
    [switch]$Force
)
begin { }
process {
    foreach ($item in $Name) {
        Write-Output $result
    }
}
end { }
```

## Function with ShouldProcess
```powershell
function Verb-Noun {
    [CmdletBinding(SupportsShouldProcess)]
    param(
        [Parameter(Mandatory, Position = 0)]
        [string]$Name,
        [Parameter(ValueFromPipelineByPropertyName)]
        [Alias('CN')]
        [string]$ComputerName = $env:COMPUTERNAME,
        [switch]$PassThru
    )
    process {
        if ($PSCmdlet.ShouldProcess($Name, 'Action')) {
            if ($PassThru) { Write-Output $result }
        }
    }
}
```

## Error Handling
```powershell
try {
    $result = Get-Content -Path $Path -ErrorAction Stop
} catch [System.IO.FileNotFoundException] {
    Write-Error "File not found: $Path"; return
} catch { throw }
```

## Splatting
```powershell
$params = @{ Path = $src; Destination = $dst; Recurse = $true; Force = $true; ErrorAction = 'Stop' }
Copy-Item @params
```

## Module Install
```powershell
# PS 5.1 (default)
Find-Module -Name 'ModuleName' -Repository PSGallery
Install-Module -Name 'ModuleName' -Scope CurrentUser -Force

# PS 7.4+ (if PSResourceGet is available)
Find-PSResource -Name 'ModuleName' -Repository PSGallery
Install-PSResource -Name 'ModuleName' -Scope CurrentUser -TrustRepository
```
