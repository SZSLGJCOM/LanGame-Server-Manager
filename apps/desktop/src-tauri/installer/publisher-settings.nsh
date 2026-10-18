; Tauri derives its install-location and language keys from the publisher.
; Read the released company's two values before its language/directory pages.
!ifndef LGSM_PUBLISHER_SETTINGS
!define LGSM_PUBLISHER_SETTINGS

!macro LGSM_COPY_PUBLISHER_SETTING value
  ReadRegStr $R0 HKCU "${MANUPRODUCTKEY}" "${value}"
  ${If} $R0 == ""
    ReadRegStr $R0 HKCU "Software\LanGame Team\${PRODUCTNAME}" "${value}"
    ${If} $R0 != ""
      ClearErrors
      WriteRegStr HKCU "${MANUPRODUCTKEY}" "${value}" $R0
      ${If} ${Errors}
        System::Store L
        SetErrorLevel 12
        Abort "$(lgsmPublisherMigrationFailed)"
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

!macro LGSM_RESTORE_PUBLISHER_SETTINGS
  System::Store S
  !insertmacro LGSM_COPY_PUBLISHER_SETTING ""
  !insertmacro LGSM_COPY_PUBLISHER_SETTING "Installer Language"
  System::Store L
  ; Tauri still applies its normal /D override and saved-location selection.
  ClearErrors
!macroend

!macro LGSM_RETIRE_PUBLISHER_SETTINGS
  System::Store S
  ReadRegStr $R0 HKCU "Software\LanGame Team\${PRODUCTNAME}" ""
  ReadRegStr $R1 HKCU "Software\LanGame Team\${PRODUCTNAME}" "Installer Language"
  ; Retire only a record positively identified as this installation.
  ${If} $R0 == $INSTDIR
    StrCpy $R2 0
    ClearErrors
    DeleteRegValue HKCU "Software\LanGame Team\${PRODUCTNAME}" ""
    IfErrors 0 +2
      StrCpy $R2 1
    ${If} $R1 != ""
      ClearErrors
      DeleteRegValue HKCU "Software\LanGame Team\${PRODUCTNAME}" "Installer Language"
      IfErrors 0 +2
        StrCpy $R2 1
    ${EndIf}
    ${If} $R2 != 0
      DetailPrint "$(lgsmPublisherCleanupDeferred)"
    ${EndIf}
    ; Unknown values and child keys are retained.
    DeleteRegKey /ifempty HKCU "Software\LanGame Team\${PRODUCTNAME}"
    DeleteRegKey /ifempty HKCU "Software\LanGame Team"
  ${EndIf}
  System::Store L
  ClearErrors
!macroend
!endif
