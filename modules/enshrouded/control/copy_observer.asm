; SHA-pinned native helper has exactly RCX/RDX/R8D inputs and no stack/FP
; inputs. A normal CALL preserves its unwind path; no return-address rewriting.
option casemap:none
EXTERN ControlCaptureCopy:PROC
EXTERN g_copyOriginal:QWORD
FRAME_BYTES EQU 0e0h
.errnz (32 + 6*16 + 7*8 + 8 + 32 - FRAME_BYTES)
PUBLIC LocalCopyBody, LocalCopyBeforeOriginal, LocalCopyAfterOriginal
PUBLIC LocalCopyFlagsPushed, LocalCopyFlagsPopped
PUBLIC LocalCopyRestoreFlagsPushed, LocalCopyRestoreFlagsPopped
PUBLIC LocalCopyEpilogueLea, LocalCopyEpiloguePop, LocalCopyEpilogueReturn
.code
ControlCopyObserver PROC FRAME
    push rbp
    .pushreg rbp
    lea rsp, [rsp-FRAME_BYTES]
    .allocstack FRAME_BYTES
    lea rbp, [rsp+FRAME_BYTES]
    .setframe rbp, FRAME_BYTES
    .endprolog
LocalCopyBody LABEL BYTE
    mov [rsp+0c0h], rcx
    mov [rsp+0c8h], rdx
    mov [rsp+0d0h], r8
    mov rax, [rbp+8]
    mov [rsp+0d8h], rax
LocalCopyBeforeOriginal LABEL BYTE
    call QWORD PTR [g_copyOriginal]
LocalCopyAfterOriginal LABEL BYTE
    mov [rsp+080h], rax
    mov [rsp+088h], rcx
    mov [rsp+090h], rdx
    mov [rsp+098h], r8
    mov [rsp+0a0h], r9
    mov [rsp+0a8h], r10
    mov [rsp+0b0h], r11
    pushfq
LocalCopyFlagsPushed LABEL BYTE
    pop rax
LocalCopyFlagsPopped LABEL BYTE
    mov [rsp+0b8h], rax
    movdqu [rsp+020h], xmm0
    movdqu [rsp+030h], xmm1
    movdqu [rsp+040h], xmm2
    movdqu [rsp+050h], xmm3
    movdqu [rsp+060h], xmm4
    movdqu [rsp+070h], xmm5
    lea rcx, [rsp+0c0h]
    mov rdx, [rsp+080h]
    call ControlCaptureCopy
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
LocalCopyRestoreFlagsPushed LABEL BYTE
    popfq
LocalCopyRestoreFlagsPopped LABEL BYTE
    mov rax, [rsp+080h]
LocalCopyEpilogueLea LABEL BYTE
    lea rsp, [rbp]
LocalCopyEpiloguePop LABEL BYTE
    pop rbp
LocalCopyEpilogueReturn LABEL BYTE
    ret
ControlCopyObserver ENDP
END
