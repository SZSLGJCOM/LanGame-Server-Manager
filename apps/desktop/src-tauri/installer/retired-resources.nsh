; Tauri /UPDATE replaces its uninstall file list without removing retired files.
; Keep retirement identities here: exact bundled paths and original LF/CRLF
; SHA256 values. Unknown or edited files are not installer-owned cleanup targets.
!ifndef LGSM_RETIRED_RESOURCES
!define LGSM_RETIRED_RESOURCES

!macro LGSM_RETIRED_RESOURCE_FUNCTION prefix
Function ${prefix}LGSMRemoveRetiredResource
  ; $0: absolute file, $1/$2: accepted hashes. Caller saves the NSIS registers.
  StrCpy $3 -1
  StrCpy $5 0
  StrCpy $6 0
  StrCpy $7 0
  StrCpy $R9 0
  ; Check the file and every ancestor, including custom installation roots.
  StrCpy $R0 $0
  lgsm_resource_path:
    System::Call 'kernel32::GetFileAttributesW(w R0) i.R1 ?e'
    Pop $R2
    IntCmp $R1 -1 lgsm_resource_missing
    IntOp $R1 $R1 & 0x400
    IntCmp $R1 0 0 lgsm_resource_preserved lgsm_resource_preserved
    ${GetParent} "$R0" $R1
    StrCmp $R1 "" lgsm_resource_open
    StrCmp $R1 $R0 lgsm_resource_open
    StrCpy $R0 $R1
    Goto lgsm_resource_path
  lgsm_resource_missing:
    IntCmp $R2 2 lgsm_resource_done
    IntCmp $R2 3 lgsm_resource_done lgsm_resource_failed lgsm_resource_failed

  lgsm_resource_open:
    ; Deny writers/deletion while checking. These retired JSON files are <2 KiB.
    System::Call 'kernel32::CreateFileW(w r0, i 0x80010000, i 1, p 0, i 3, i 0x00200000, p 0) p.r3'
    IntCmp $3 -1 lgsm_resource_failed
    System::Call 'kernel32::GetFinalPathNameByHandleW(p r3, w.R0, i ${NSIS_MAX_STRLEN}, i 0) i.R1'
    IntCmp $R1 0 lgsm_resource_failed
    IntCmp $R1 ${NSIS_MAX_STRLEN} lgsm_resource_preserved 0 lgsm_resource_preserved
    StrCpy $R2 "\\?\$0"
    StrCmp $R0 $R2 0 lgsm_resource_preserved
    System::Call 'kernel32::GetFileSize(p r3, *i.R2) i.r4 ?e'
    Pop $R0
    ${If} $4 = -1
    ${AndIf} $R0 <> 0
      Goto lgsm_resource_failed
    ${EndIf}
    IntCmp $R2 0 0 lgsm_resource_preserved lgsm_resource_preserved
    IntCmpU $4 65536 lgsm_resource_preserved 0 lgsm_resource_preserved
    IntCmp $4 0 lgsm_resource_preserved lgsm_resource_failed
    System::Alloc $4
    Pop $5
    IntCmp $5 0 lgsm_resource_failed
    System::Call 'kernel32::ReadFile(p r3, p r5, i r4, *i.R0, p 0) i.r8'
    IntCmp $8 0 lgsm_resource_failed
    IntCmp $R0 $4 0 lgsm_resource_failed lgsm_resource_failed
    System::Alloc 32
    Pop $6
    IntCmp $6 0 lgsm_resource_failed
    System::Call 'bcrypt::BCryptOpenAlgorithmProvider(*p.r7, w "SHA256", p 0, i 0) i.r8'
    IntCmp $8 0 0 lgsm_resource_failed lgsm_resource_failed
    System::Call 'bcrypt::BCryptHash(p r7, p 0, i 0, p r5, i r4, p r6, i 32) i.r8'
    IntCmp $8 0 0 lgsm_resource_failed lgsm_resource_failed
    System::Call 'crypt32::CryptBinaryToStringW(p r6, i 32, i 0x4000000c, w.r9, *i 65) i.r8'
    IntCmp $8 0 lgsm_resource_failed
    StrCmp $9 $1 lgsm_resource_remove
    StrCmp $9 $2 0 lgsm_resource_preserved
  lgsm_resource_remove:
    ; Delete through the checked handle, without a close/reopen path race.
    System::Call 'kernel32::SetFileInformationByHandle(p r3, i 4, *i 1, i 4) i.r8'
    IntCmp $8 0 lgsm_resource_failed
    System::Call 'kernel32::CloseHandle(p r3)'
    StrCpy $3 -1
    DetailPrint "Removed retired package resource: $0"
    ${GetParent} "$0" $R0
    ; Remove only an empty directory whose ordinary ancestors were checked.
    RMDir "$R0"
    Goto lgsm_resource_done
  lgsm_resource_preserved:
    DetailPrint "Preserved non-package resource: $0"
    Goto lgsm_resource_done
  lgsm_resource_failed:
    StrCpy $R9 1
  lgsm_resource_done:
    IntCmp $3 -1 +2
      System::Call 'kernel32::CloseHandle(p r3)'
    IntCmp $7 0 +2
      System::Call 'bcrypt::BCryptCloseAlgorithmProvider(p r7, i 0)'
    System::Free $6
    System::Free $5
    Push $R9
FunctionEnd
!macroend

!insertmacro LGSM_RETIRED_RESOURCE_FUNCTION ""
!insertmacro LGSM_RETIRED_RESOURCE_FUNCTION "un."

!macro LGSM_REMOVE_RETIRED_RESOURCE prefix relative hashLF hashCRLF
  System::Store S
  StrCpy $0 "$INSTDIR\${relative}"
  StrCpy $1 "${hashLF}"
  StrCpy $2 "${hashCRLF}"
  Call ${prefix}LGSMRemoveRetiredResource
  Pop $R0
  ${If} $R0 <> 0
    DetailPrint "$(lgsmResourceCleanupFailed): $0"
    System::Store L
    SetErrorLevel 11
    Abort "$(lgsmResourceCleanupFailed)"
  ${EndIf}
  System::Store L
!macroend

!macro LGSM_REMOVE_RETIRED_RESOURCES prefix
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\enshrouded\config-fixtures\2026-09-30-native_ban_contract_build_23178631.json" "a7e2731217a7522f6d28bf0355cdfa098253d1d90e2c6d4c1594e407de239719" "ccff88d90223848426dc7ac8ecb0c9969d497b1ee886c07e9b28303a52810b60"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCE "${prefix}" "modules\terraria\config-fixtures\2026-09-28-official_dedicated_zip_1458.json" "55860a78dc8e8640923556b8469ab9d375b8c82fc36e815a9dd9908160d75ec5" "fd5de98799c4655ee34dc4c232f58a804fcdab066be290b6e7cda1ba56c8b873"
  ClearErrors
!macroend
!endif
