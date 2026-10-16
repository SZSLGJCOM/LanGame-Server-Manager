option casemap:none
.code
UnwindUpdateTarget PROC FRAME
    db 048h, 08bh, 0c4h
    push rbp
    .pushreg rbp
    push rbx
    .pushreg rbx
    .endprolog
    pop rbx
    pop rbp
    mov rax, 012345678h
    ret
UnwindUpdateTarget ENDP
UnwindCopyTarget PROC FRAME
    db 040h, 053h
    .pushreg rbx
    sub rsp, 020h
    .allocstack 020h
    .endprolog
    lea rsp, [rsp+020h]
    pop rbx
    mov rax, 087654321h
    ret
UnwindCopyTarget ENDP
END
