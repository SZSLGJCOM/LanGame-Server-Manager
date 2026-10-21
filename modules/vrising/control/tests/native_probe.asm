option casemap:none
extern lgsm_probe_body:proc
.code
public lgsm_probe_trigger
lgsm_probe_trigger proc frame
    db 040h
    push rbp
    .pushreg rbp
    push rbx
    .pushreg rbx
    push r12
    .pushreg r12
    sub rsp, 20h
    .allocstack 20h
    .endprolog
    call lgsm_probe_body
    add rsp, 20h
    pop r12
    pop rbx
    pop rbp
    ret
lgsm_probe_trigger endp
public lgsm_probe_serialize
lgsm_probe_serialize proc frame
    mov qword ptr [rsp+8],rcx
    sub rsp, 28h
    .allocstack 28h
    .endprolog
    call lgsm_probe_body
    add rsp, 28h
    ret
lgsm_probe_serialize endp
public lgsm_probe_finalize
lgsm_probe_finalize proc frame
    db 040h
    push rbx
    .pushreg rbx
    sub rsp, 20h
    .allocstack 20h
    .endprolog
    call lgsm_probe_body
    add rsp, 20h
    pop rbx
    ret
lgsm_probe_finalize endp
end
