param(
    [Parameter(Mandatory = $true)]
    [string] $Path,

    [switch] $StrictSystemOnly
)

$ErrorActionPreference = "Stop"

$FullPath = (Resolve-Path -LiteralPath $Path).Path
$Bytes = [System.IO.File]::ReadAllBytes($FullPath)

function Read-U16([int] $Offset) {
    return [System.BitConverter]::ToUInt16($Bytes, $Offset)
}

function Read-U32([int] $Offset) {
    return [System.BitConverter]::ToUInt32($Bytes, $Offset)
}

function Read-AsciiZ([int] $Offset) {
    $End = $Offset
    while ($End -lt $Bytes.Length -and $Bytes[$End] -ne 0) {
        $End++
    }

    if ($End -ge $Bytes.Length) {
        throw "Unterminated string at file offset $Offset"
    }

    return [System.Text.Encoding]::ASCII.GetString($Bytes, $Offset, $End - $Offset)
}

function Assert-Range([int64] $Offset, [int64] $Length, [string] $What) {
    if ($Offset -lt 0 -or $Length -lt 0 -or ($Offset + $Length) -gt $Bytes.Length) {
        throw "$What is outside the PE file"
    }
}

Assert-Range 0 0x40 "DOS header"
if ($Bytes[0] -ne 0x4D -or $Bytes[1] -ne 0x5A) {
    throw "$FullPath is not a Windows PE executable (missing MZ header)"
}

$PeOffset = [int] (Read-U32 0x3C)
Assert-Range $PeOffset 0x18 "PE header"
if ($Bytes[$PeOffset] -ne 0x50 -or $Bytes[$PeOffset + 1] -ne 0x45 -or $Bytes[$PeOffset + 2] -ne 0 -or $Bytes[$PeOffset + 3] -ne 0) {
    throw "$FullPath is not a Windows PE executable (missing PE header)"
}

$CoffOffset = $PeOffset + 4
$SectionCount = Read-U16 ($CoffOffset + 2)
$OptionalHeaderSize = Read-U16 ($CoffOffset + 16)
$OptionalOffset = $CoffOffset + 20
Assert-Range $OptionalOffset $OptionalHeaderSize "optional header"

$Magic = Read-U16 $OptionalOffset
switch ($Magic) {
    0x10B { $DataDirectoryOffset = $OptionalOffset + 96 }
    0x20B { $DataDirectoryOffset = $OptionalOffset + 112 }
    default { throw "Unsupported PE optional-header magic: 0x$($Magic.ToString('X'))" }
}

Assert-Range $DataDirectoryOffset 16 "PE data directories"
$ImportRva = Read-U32 ($DataDirectoryOffset + 8)

$SectionTableOffset = $OptionalOffset + $OptionalHeaderSize
Assert-Range $SectionTableOffset (40 * $SectionCount) "section table"

$Sections = @()
for ($Index = 0; $Index -lt $SectionCount; $Index++) {
    $Offset = $SectionTableOffset + (40 * $Index)
    $Sections += [PSCustomObject]@{
        VirtualSize      = Read-U32 ($Offset + 8)
        VirtualAddress   = Read-U32 ($Offset + 12)
        SizeOfRawData    = Read-U32 ($Offset + 16)
        PointerToRawData = Read-U32 ($Offset + 20)
    }
}

function Convert-RvaToOffset([uint32] $Rva) {
    foreach ($Section in $Sections) {
        $Start = [uint32] $Section.VirtualAddress
        $Span = [Math]::Max([uint32] $Section.VirtualSize, [uint32] $Section.SizeOfRawData)
        $End = $Start + $Span

        if ($Rva -ge $Start -and $Rva -lt $End) {
            $Offset = [int64] $Section.PointerToRawData + ([int64] $Rva - [int64] $Start)
            Assert-Range $Offset 1 "RVA 0x$($Rva.ToString('X'))"
            return [int] $Offset
        }
    }

    if ($Rva -lt $Bytes.Length) {
        return [int] $Rva
    }

    throw "Could not map RVA 0x$($Rva.ToString('X')) to a file offset"
}

$Imports = New-Object System.Collections.Generic.List[string]
if ($ImportRva -ne 0) {
    $DescriptorOffset = Convert-RvaToOffset $ImportRva

    while ($true) {
        Assert-Range $DescriptorOffset 20 "import descriptor"

        $OriginalFirstThunk = Read-U32 $DescriptorOffset
        $TimeDateStamp = Read-U32 ($DescriptorOffset + 4)
        $ForwarderChain = Read-U32 ($DescriptorOffset + 8)
        $NameRva = Read-U32 ($DescriptorOffset + 12)
        $FirstThunk = Read-U32 ($DescriptorOffset + 16)

        if (($OriginalFirstThunk -bor $TimeDateStamp -bor $ForwarderChain -bor $NameRva -bor $FirstThunk) -eq 0) {
            break
        }

        $NameOffset = Convert-RvaToOffset $NameRva
        $Imports.Add((Read-AsciiZ $NameOffset))
        $DescriptorOffset += 20
    }
}

$UniqueImports = $Imports | Sort-Object -Unique
Write-Host "Windows imports for $([System.IO.Path]::GetFileName($FullPath)):"
foreach ($Import in $UniqueImports) {
    Write-Host "  $Import"
}

$AllowedSystemDllPattern = '^(advapi32|bcrypt|bcryptprimitives|cfgmgr32|comctl32|comdlg32|crypt32|d2d1|d3d11|d3d12|d3d9|dbghelp|dwmapi|dxgi|gdi32|hid|imm32|iphlpapi|kernel32|mpr|msimg32|msvcrt|ncrypt|normaliz|ntdll|ole32|oleacc|oleaut32|opengl32|propsys|rpcrt4|runtimeobject|secur32|setupapi|shcore|shell32|shlwapi|user32|userenv|uxtheme|version|windowscodecs|winhttp|wininet|winmm|winspool\.drv|ws2_32|wtsapi32)\.dll$|^(api-ms-win-|ext-ms-win-)'
$BlockedRuntimePattern = '^(libunwind|libgcc|libstdc\+\+|libwinpthread|libc\+\+|libc\+\+abi|zlib1)\.dll$'

$Unexpected = @()
foreach ($Import in $UniqueImports) {
    $Lower = $Import.ToLowerInvariant()
    if ($Lower -match $BlockedRuntimePattern) {
        $Unexpected += $Import
        continue
    }

    if ($StrictSystemOnly -and $Lower -notmatch $AllowedSystemDllPattern) {
        $Unexpected += $Import
    }
}

if ($Unexpected.Count -gt 0) {
    Write-Error ("Unexpected non-system DLL imports found:`n  " + ($Unexpected -join "`n  ") + "`nThe Windows release must be self-contained. Rebuild with static CRT/runtime settings.")
    exit 1
}
