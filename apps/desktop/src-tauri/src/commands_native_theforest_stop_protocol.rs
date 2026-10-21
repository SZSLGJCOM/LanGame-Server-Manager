//! Closed transcript contract for the disposable native lifecycle probe.
const CONTROL: &str = "[LanGame native control] ";

#[derive(Debug, PartialEq, Eq)]
enum Event {
    NonceSave,
    PlainSave,
    InputStopped,
    ShutdownRequested,
}

fn positive_bytes(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<i64>().is_ok_and(|bytes| bytes > 0)
}

pub(super) fn validate(text: &str, manual_save: bool) -> Result<(), &'static str> {
    let mut events = Vec::new();
    for line in text.lines() {
        let Some(message) = line.strip_prefix(CONTROL) else {
            continue;
        };
        if let Some(receipt) = message.strip_prefix("native_save_completed ") {
            if let Some(bytes) = receipt.strip_prefix("bytes=") {
                if !positive_bytes(bytes) {
                    return Err("The Forest returned invalid native checkpoint bytes");
                }
                events.push(Event::PlainSave);
            } else if let Some((nonce, bytes)) = receipt
                .strip_prefix("nonce=")
                .and_then(|value| value.split_once(" bytes="))
            {
                if nonce.len() != 32
                    || !nonce
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                    || !positive_bytes(bytes)
                {
                    return Err("The Forest returned a malformed request-bound checkpoint receipt");
                }
                events.push(Event::NonceSave);
            } else {
                return Err("The Forest returned a malformed native checkpoint receipt");
            }
        } else if message.starts_with("native_save_completed")
            || message.starts_with("native_save_failed")
            || message.starts_with("command failed:")
            || message.starts_with("command refused:")
        {
            return Err("The Forest reported failed or malformed native control completion");
        } else if message == "input_reader_stopped" {
            events.push(Event::InputStopped);
        } else if message == "native_shutdown_requested" {
            events.push(Event::ShutdownRequested);
        }
    }
    let first = if manual_save {
        Event::NonceSave
    } else {
        Event::PlainSave
    };
    if events
        != [
            first,
            Event::PlainSave,
            Event::InputStopped,
            Event::ShutdownRequested,
        ]
    {
        return Err(
            "The Forest stop did not retain its exact ordered save/input/shutdown transcript",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const NONCE: &str = "000000000075fc39e0a9bf6cfd3244ce";
    fn frame(first: &str) -> String {
        format!(
            "{CONTROL}world_ready=True\n{CONTROL}{first}\n{CONTROL}native_save_completed bytes=196180\n{CONTROL}world_ready=False\n{CONTROL}input_reader_stopped\n{CONTROL}native_shutdown_requested\n"
        )
    }
    fn manual() -> String {
        frame(&format!("native_save_completed nonce={NONCE} bytes=196180"))
    }

    #[test]
    fn manual_save_requires_the_observed_nonce_completion_then_plain_shutdown_save() {
        assert!(validate(&manual(), true).is_ok());
        assert!(validate(&manual(), false).is_err());
        let legacy = frame("native_save_completed bytes=196180");
        assert!(validate(&legacy, false).is_ok());
        assert!(validate(&legacy, true).is_err());
    }

    #[test]
    fn duplicate_missing_or_reordered_events_do_not_establish_normal_stop() {
        let good = manual();
        for message in [
            format!("native_save_completed nonce={NONCE} bytes=196180"),
            "native_save_completed bytes=196180".into(),
            "input_reader_stopped".into(),
            "native_shutdown_requested".into(),
        ] {
            let line = format!("{CONTROL}{message}\n");
            assert!(validate(&good.replace(&line, ""), true).is_err());
            assert!(validate(&(good.clone() + &line), true).is_err());
        }
        let reordered = good.replace(
            &format!("{CONTROL}input_reader_stopped\n{CONTROL}native_shutdown_requested"),
            &format!("{CONTROL}native_shutdown_requested\n{CONTROL}input_reader_stopped"),
        );
        assert!(validate(&reordered, true).is_err());
        assert!(
            validate(
                &good.replace(
                    "native_save_completed bytes=196180",
                    "native_save_completed nonce=000000000075fc39aaaaaaaaaaaaaaaa bytes=1"
                ),
                true
            )
            .is_err()
        );
    }

    #[test]
    fn invalid_nonce_lengths_case_and_bytes_cannot_supply_completion() {
        for nonce in [
            "short",
            "000000000075FC39E0A9BF6CFD3244CE",
            "000000000075fc39e0a9bf6cfd3244cex",
        ] {
            assert!(
                validate(
                    &frame(&format!("native_save_completed nonce={nonce} bytes=1")),
                    true
                )
                .is_err()
            );
        }
        for bytes in [
            "",
            "0",
            "-1",
            "+1",
            "1 trailing",
            "1 ",
            "9223372036854775808",
        ] {
            assert!(
                validate(
                    &frame(&format!(
                        "native_save_completed nonce={NONCE} bytes={bytes}"
                    )),
                    true
                )
                .is_err()
            );
            assert!(
                validate(
                    &frame(&format!("native_save_completed bytes={bytes}")),
                    false
                )
                .is_err()
            );
        }
    }

    #[test]
    fn native_failure_and_unowned_lookalikes_never_satisfy_the_contract() {
        for failure in [
            "native_save_failed nonce=000000000075fc39e0a9bf6cfd3244ce code=expired",
            "command failed: invoke IOException",
            "command refused: world is not ready",
        ] {
            assert!(validate(&(manual() + CONTROL + failure + "\n"), true).is_err());
        }
        assert!(
            validate(
                &manual().replace(CONTROL, "player: [LanGame native control] "),
                true
            )
            .is_err()
        );
    }
}
