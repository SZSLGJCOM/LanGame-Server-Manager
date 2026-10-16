; Independent Windows x64 entry observer. The original stack and return address
; are restored before tail-jumping; no unknown native function ABI is declared.
option casemap:none
EXTERN ControlCaptureUpdate:PROC
EXTERN g_updateOriginal:QWORD
FRAME_BYTES EQU (32 + 6*16 + 7*8 + 8)
.errnz (FRAME_BYTES - 0c0h)
PUBLIC ObserverBody, ObserverFlagsPushed, ObserverFlagsPopped
PUBLIC ObserverRestoreFlagsPushed, ObserverRestoreFlagsPopped
PUBLIC ObserverEpilogueLea, ObserverEpiloguePop, ObserverEpilogueJump
.code
EntryObserver MACRO EntryName, CallbackName, OriginalName, Markers
EntryName PROC FRAME
    push rbp
    .pushreg rbp
    lea rsp, [rsp-FRAME_BYTES]
    .allocstack FRAME_BYTES
    lea rbp, [rsp+FRAME_BYTES]
    .setframe rbp, FRAME_BYTES
    .endprolog
IF Markers
    ObserverBody LABEL BYTE
ENDIF
    mov [rsp+080h], rax
    mov [rsp+088h], rcx
    mov [rsp+090h], rdx
    mov [rsp+098h], r8
    mov [rsp+0a0h], r9
    mov [rsp+0a8h], r10
    mov [rsp+0b0h], r11
    pushfq
IF Markers
    ObserverFlagsPushed LABEL BYTE
ENDIF
    pop rax
IF Markers
    ObserverFlagsPopped LABEL BYTE
ENDIF
    mov [rsp+0b8h], rax
    movdqu [rsp+020h], xmm0
    movdqu [rsp+030h], xmm1
    movdqu [rsp+040h], xmm2
    movdqu [rsp+050h], xmm3
    movdqu [rsp+060h], xmm4
    movdqu [rsp+070h], xmm5
    call CallbackName
    movdqu xmm0, [rsp+020h]
    movdqu xmm1, [rsp+030h]
    movdqu xmm2, [rsp+040h]
    movdqu xmm3, [rsp+050h]
    movdqu xmm4, [rsp+060h]
    movdqu xmm5, [rsp+070h]
    mov rcx, [rsp+088h]
    mov rdx, [rsp+090h]
    mov r8, [rsp+098h]
    mov r9, [rsp+0a0h]
    mov r10, [rsp+0a8h]
    mov r11, [rsp+0b0h]
    mov rax, [rsp+0b8h]
    push rax
IF Markers
    ObserverRestoreFlagsPushed LABEL BYTE
ENDIF
    popfq
IF Markers
    ObserverRestoreFlagsPopped LABEL BYTE
ENDIF
    mov rax, [rsp+080h]
IF Markers
    ObserverEpilogueLea LABEL BYTE
ENDIF
    lea rsp, [rbp]
IF Markers
    ObserverEpiloguePop LABEL BYTE
ENDIF
    pop rbp
IF Markers
    ObserverEpilogueJump LABEL BYTE
ENDIF
    jmp QWORD PTR [OriginalName]
EntryName ENDP
ENDM
EntryObserver ControlUpdateObserver, ControlCaptureUpdate, g_updateOriginal, 1
END
