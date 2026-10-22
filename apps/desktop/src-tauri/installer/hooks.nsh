; Tauri CLI 2.12.3 includes installerHooks after utils.nsh. Replace its process
; guard instead of allowing its later check to kill a newly started process.
; The desktop and runtime service share an executable name. Neither may be
; force-terminated by installation or removal: servers must save and stop first.
!include FileFunc.nsh
!include "${__FILEDIR__}\publisher-settings.nsh"

; Tauri invokes this macro in .onInit, before any install hook. MUI's default
; skips its dialog for /S but not /P, so an older updater can wait forever when
; a silent first install never saved a language. Keep manual language selection
; and saved preferences; passive/silent installs use NSIS's OS-language default.
!ifmacrondef MUI_LANGDLL_DISPLAY
  !error "Unsupported Tauri NSIS template: language initialization is missing."
!endif
!macroundef MUI_LANGDLL_DISPLAY

!macro MUI_LANGDLL_DISPLAY
  !insertmacro LGSM_RESTORE_PUBLISHER_SETTINGS
  !define LGSM_LANGUAGE_ID ${__LINE__}
  Push $R0
  ReadRegStr $R0 "${MUI_LANGDLL_REGISTRY_ROOT}" "${MUI_LANGDLL_REGISTRY_KEY}" "${MUI_LANGDLL_REGISTRY_VALUENAME}"
  StrCmp $R0 "" lgsm_language_default_${LGSM_LANGUAGE_ID}
  StrCpy $LANGUAGE $R0
  Goto lgsm_language_ready_${LGSM_LANGUAGE_ID}

  lgsm_language_default_${LGSM_LANGUAGE_ID}:
    IfSilent lgsm_language_ready_${LGSM_LANGUAGE_ID}
    StrCmp $PassiveMode 1 lgsm_language_ready_${LGSM_LANGUAGE_ID}
    ; The language/codepage list comes from the configured MUI_LANGUAGE entries.
    LangDLL::LangDialog "Installer Language" "Please select a language." AC ${MUI_LANGDLL_LANGUAGES_CP} ""
    Pop $R0
    StrCmp $R0 "cancel" 0 lgsm_language_selected_${LGSM_LANGUAGE_ID}
    Pop $R0
    Abort

  lgsm_language_selected_${LGSM_LANGUAGE_ID}:
    StrCpy $LANGUAGE $R0
  lgsm_language_ready_${LGSM_LANGUAGE_ID}:
    Pop $R0
  !undef LGSM_LANGUAGE_ID
!macroend

!include "${__FILEDIR__}\retired-resources.nsh"

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCES ""
  !insertmacro LGSM_RETIRE_PUBLISHER_SETTINGS
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; This hook precedes Tauri's own guard, so check before touching any files.
  !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
  !insertmacro LGSM_REMOVE_RETIRED_RESOURCES "un."
!macroend

!ifmacrondef CheckIfAppIsRunning
  !error "Unsupported Tauri NSIS template: process guard is missing."
!endif
!macroundef CheckIfAppIsRunning

!macro CheckIfAppIsRunning executablePath productName
  !define LGSM_GUARD_ID ${__LINE__}
  Push $R0
  Push $R1
  Push $R2
  Push $R3
  ; Tauri passes a full installation path; the process plugin matches names.
  ; Keep guarding every same-user runtime, including another installation.
  ${GetFileName} "${executablePath}" $R2
  StrCpy $R1 50

  lgsm_probe_${LGSM_GUARD_ID}:
    ClearErrors
    nsis_tauri_utils::FindProcessCurrentUser "$R2"
    Pop $R0
    IfErrors lgsm_probe_failed_${LGSM_GUARD_ID}
    StrCmp $R0 1 lgsm_file_probe_${LGSM_GUARD_ID}
    StrCmp $R0 0 0 lgsm_probe_failed_${LGSM_GUARD_ID}
    ; The updater stops the runtime before launching NSIS, then exits itself.
    ; Wait only for that documented parent-exit handoff, at most ten seconds.
    ; /UPDATE alone is not evidence that the service has stopped.
    StrCmp $UpdateMode 1 0 lgsm_busy_${LGSM_GUARD_ID}
    IntCmp $R1 0 lgsm_busy_${LGSM_GUARD_ID}
    Sleep 200
    IntOp $R1 $R1 - 1
    Goto lgsm_probe_${LGSM_GUARD_ID}

  lgsm_file_probe_${LGSM_GUARD_ID}:
    StrCmp $UpdateMode 1 0 lgsm_ready_${LGSM_GUARD_ID}
    ; Process enumeration can finish before the old image's write lock clears.
    ; Match NSIS's GENERIC_WRITE/FILE_SHARE_READ; OPEN_EXISTING never truncates.
    System::Call 'kernel32::CreateFileW(w "${executablePath}", i 0x40000000, i 1, p 0, i 3, i 0x80, p 0) p .R0 ?e'
    Pop $R3
    StrCmp $R0 -1 lgsm_file_unavailable_${LGSM_GUARD_ID}
    System::Call 'kernel32::CloseHandle(p R0)'
    Goto lgsm_ready_${LGSM_GUARD_ID}

  lgsm_file_unavailable_${LGSM_GUARD_ID}:
    StrCmp $R3 2 lgsm_ready_${LGSM_GUARD_ID} ; no existing file
    StrCmp $R3 3 lgsm_ready_${LGSM_GUARD_ID} ; no existing directory
    ; Access denial may be a transient image lock or a permanent permission.
    ; Both share the existing handoff budget; other errors fail immediately.
    StrCmp $R3 5 lgsm_file_wait_${LGSM_GUARD_ID}
    StrCmp $R3 32 lgsm_file_wait_${LGSM_GUARD_ID}
    StrCmp $R3 33 lgsm_file_wait_${LGSM_GUARD_ID} lgsm_file_failed_${LGSM_GUARD_ID}
  lgsm_file_wait_${LGSM_GUARD_ID}:
    IntCmp $R1 0 lgsm_file_failed_${LGSM_GUARD_ID}
    Sleep 200
    IntOp $R1 $R1 - 1
    Goto lgsm_probe_${LGSM_GUARD_ID}
  lgsm_file_failed_${LGSM_GUARD_ID}:
    DetailPrint "Cannot open existing application for replacement (Windows error $R3)."
    StrCpy $R0 "$(lgsmFileNotWritable)"
    Goto lgsm_refuse_${LGSM_GUARD_ID}

  lgsm_busy_${LGSM_GUARD_ID}:
    StrCpy $R0 "$(lgsmCloseBeforeMaintenance)"
    Goto lgsm_refuse_${LGSM_GUARD_ID}
  lgsm_probe_failed_${LGSM_GUARD_ID}:
    StrCpy $R0 "$(lgsmProcessCheckFailed)"
  lgsm_refuse_${LGSM_GUARD_ID}:
    DetailPrint $R0
    IfSilent lgsm_abort_${LGSM_GUARD_ID}
    StrCmp $PassiveMode 1 lgsm_abort_${LGSM_GUARD_ID}
    MessageBox MB_OK|MB_ICONEXCLAMATION $R0
  lgsm_abort_${LGSM_GUARD_ID}:
    Pop $R3
    Pop $R2
    Pop $R1
    Pop $R0
    SetErrorLevel 10
    Abort

  lgsm_ready_${LGSM_GUARD_ID}:
    Pop $R3
    Pop $R2
    Pop $R1
    Pop $R0
  !undef LGSM_GUARD_ID
!macroend
