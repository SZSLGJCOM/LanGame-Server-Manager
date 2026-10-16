option casemap:none
EXTERN g_observedRcx:QWORD
EXTERN g_callbackCount:QWORD
EXTERN ControlUpdateObserver:PROC
.code
ControlCaptureUpdate PROC
    mov [g_observedRcx], rcx
    inc QWORD PTR [g_callbackCount]
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
ControlCaptureUpdate ENDP
PrototypePublicationCapture PROC
    jmp ControlCaptureUpdate
PrototypePublicationCapture ENDP
ProbeEntryAlignment PROC
    mov rax, rsp
    and rax, 0fh
    ret
ProbeEntryAlignment ENDP
ProbeCarry PROC
    mov rax, 0
    adc rax, 0
    ret
ProbeCarry ENDP
CallObserverWithCarry PROC FRAME
    sub rsp, 028h
    .allocstack 028h
    .endprolog
    stc
    call ControlUpdateObserver
    lea rsp, [rsp+028h]
    ret
CallObserverWithCarry ENDP
END
