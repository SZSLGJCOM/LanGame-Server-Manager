/// Windows process exit codes preserve the native exception's unsigned bits.
/// Recognize documented crash statuses explicitly; arbitrary nonzero game codes
/// (including ARK's -1) and STATUS_CONTROL_C_EXIT are not crash evidence.
/// Reference: Microsoft's MS-ERREF NTSTATUS Values specification.
pub(super) fn windows_crash_exit_reason(exit_code: Option<i32>) -> Option<&'static str> {
    match exit_code? as u32 {
        0x8000_0001 => Some("STATUS_GUARD_PAGE_VIOLATION"),
        0x8000_0002 => Some("STATUS_DATATYPE_MISALIGNMENT"),
        0x8000_0003 => Some("STATUS_BREAKPOINT"),
        0x8000_0004 => Some("STATUS_SINGLE_STEP"),
        0xC000_0005 => Some("STATUS_ACCESS_VIOLATION"),
        0xC000_0006 => Some("STATUS_IN_PAGE_ERROR"),
        0xC000_001D => Some("STATUS_ILLEGAL_INSTRUCTION"),
        0xC000_0025 => Some("STATUS_NONCONTINUABLE_EXCEPTION"),
        0xC000_0026 => Some("STATUS_INVALID_DISPOSITION"),
        0xC000_008C => Some("STATUS_ARRAY_BOUNDS_EXCEEDED"),
        0xC000_008D => Some("STATUS_FLOAT_DENORMAL_OPERAND"),
        0xC000_008E => Some("STATUS_FLOAT_DIVIDE_BY_ZERO"),
        0xC000_008F => Some("STATUS_FLOAT_INEXACT_RESULT"),
        0xC000_0090 => Some("STATUS_FLOAT_INVALID_OPERATION"),
        0xC000_0091 => Some("STATUS_FLOAT_OVERFLOW"),
        0xC000_0092 => Some("STATUS_FLOAT_STACK_CHECK"),
        0xC000_0093 => Some("STATUS_FLOAT_UNDERFLOW"),
        0xC000_0094 => Some("STATUS_INTEGER_DIVIDE_BY_ZERO"),
        0xC000_0095 => Some("STATUS_INTEGER_OVERFLOW"),
        0xC000_0096 => Some("STATUS_PRIVILEGED_INSTRUCTION"),
        0xC000_00FD => Some("STATUS_STACK_OVERFLOW"),
        0xC000_0374 => Some("STATUS_HEAP_CORRUPTION"),
        0xC000_0409 => Some("STATUS_STACK_BUFFER_OVERRUN"),
        0xC000_0417 => Some("STATUS_INVALID_CRUNTIME_PARAMETER"),
        0xC000_0420 => Some("STATUS_ASSERTION_FAILURE"),
        0xC000_0602 => Some("STATUS_FAIL_FAST_EXCEPTION"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::windows_crash_exit_reason;

    #[test]
    fn native_exit_classification_rejects_windows_crash_statuses() {
        assert_eq!(
            windows_crash_exit_reason(Some(-2147483645)),
            Some("STATUS_BREAKPOINT")
        );
        for code in [
            0xC000_0005_u32,
            0xC000_00FD,
            0xC000_0374,
            0xC000_0409,
            0xC000_0602,
        ] {
            assert!(
                windows_crash_exit_reason(Some(code as i32)).is_some(),
                "0x{code:08X}"
            );
        }
    }

    #[test]
    fn native_exit_classification_does_not_treat_every_nonzero_code_as_a_crash() {
        for code in [
            None,
            Some(0),
            Some(1),
            Some(-1),
            Some(-1073741510),
            Some(0x8000_0005_u32 as i32),
        ] {
            assert_eq!(windows_crash_exit_reason(code), None, "{code:?}");
        }
    }
}
