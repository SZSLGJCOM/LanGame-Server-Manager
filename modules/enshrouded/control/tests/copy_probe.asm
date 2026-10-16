option casemap:none
EXTERN g_localContext:BYTE
EXTERN g_localResult:QWORD
EXTERN g_localCallbacks:QWORD
EXTERN g_localCarry:QWORD
EXTERN ControlCopyObserver:PROC
.code
ControlCaptureCopy PROC
    mov rax, [rcx]
    mov QWORD PTR [g_localContext], rax
    mov rax, [rcx+8]
    mov QWORD PTR [g_localContext+8], rax
    mov rax, [rcx+10h]
    mov QWORD PTR [g_localContext+10h], rax
    mov rax, [rcx+18h]
    mov QWORD PTR [g_localContext+18h], rax
    mov [g_localResult], rdx
    inc QWORD PTR [g_localCallbacks]
    xor rax, rax
    xor rcx, rcx
    xor rdx, rdx
    xor r8, r8
    xor r9, r9
    xor r10, r10
    xor r11, r11
    pxor xmm0, xmm0
    pxor xmm1, xmm1
    pxor xmm2, xmm2
    pxor xmm3, xmm3
    pxor xmm4, xmm4
    pxor xmm5, xmm5
    clc
    ret
ControlCaptureCopy ENDP
LocalCopyProbeTarget PROC
    mov [rcx], r8d
    mov [rcx+8], rdx
    mov rax, 03ff4000000000000h
    movq xmm0, rax
    cmp r8d, 0ffffffffh
    jne success
    or rax, 01fh
success:
    stc
    ret
LocalCopyProbeTarget ENDP
CallLocalCopyWithCarry PROC FRAME
    sub rsp, 028h
    .allocstack 028h
    .endprolog
    call ControlCopyObserver
    mov r10, 0
    adc r10, 0
    mov [g_localCarry], r10
    lea rsp, [rsp+028h]
    ret
CallLocalCopyWithCarry ENDP
END
