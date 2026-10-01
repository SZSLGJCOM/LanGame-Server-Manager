use super::*;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Instant;

const COMMAND: &str = "ban add EOS_deadbeef 10 years LanGame";
const PRIMARY: &str = "EOS_deadbeef banned until 2036-09-28 00:12:34, reason: LanGame.\r\n";
const OWNER: &str = "Steam Family Sharing license owner Steam_76561198000000002 banned until 2036-09-28 00:12:34, reason: LanGame.\r\n";

fn line(reader: &mut BufReader<TcpStream>) -> String {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(!line.is_empty(), "fixture expected another client line");
    line.trim_end_matches(['\r', '\n']).to_string()
}

fn with_peer(
    budget: Duration,
    serve: impl FnOnce(TcpStream) + Send + 'static,
) -> Result<String, String> {
    // Synthetic loopback peer only; never resolves a game server endpoint.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let worker = thread::spawn(move || {
        let (peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        peer.set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        serve(peer);
    });
    let mut stream =
        DeadlineTcpStream::connect_with_budget(&endpoint, Duration::from_millis(20), budget)
            .unwrap();
    let result = exchange(&mut stream, "synthetic-password", COMMAND);
    let _ = stream.shutdown();
    worker.join().unwrap();
    result
}

fn authenticate_and_read_command(peer: &mut TcpStream) -> (BufReader<TcpStream>, String) {
    let mut reader = BufReader::new(peer.try_clone().unwrap());
    assert_eq!(line(&mut reader), "synthetic-password");
    peer.write_all(b"Please enter password:\r\nLogon successful.\r\n")
        .unwrap();
    let begin = line(&mut reader);
    assert!(begin.starts_with("lsgm_ban_begin_"));
    peer.write_all(format!("banner\r\n{}\r\n", marker_response(&begin)).as_bytes())
        .unwrap();
    assert_eq!(line(&mut reader), COMMAND);
    let end = line(&mut reader);
    assert!(end.starts_with("lsgm_ban_end_"));
    assert_eq!(
        begin.strip_prefix("lsgm_ban_begin_"),
        end.strip_prefix("lsgm_ban_end_")
    );
    (reader, end)
}

#[test]
fn seven_days_telnet_waits_for_delayed_owner_and_ordered_completion_after_normal_logs() {
    let response = with_peer(Duration::from_secs(2), |mut peer| {
        let (_, end) = authenticate_and_read_command(&mut peer);
        peer.write_all(PRIMARY.as_bytes()).unwrap();
        // Longer than the client's idle timeout: silence must not end the ban.
        thread::sleep(Duration::from_millis(90));
        peer.write_all(b"2026-09-28T00:12:34 1.000 INF Player disconnected\r\n")
            .unwrap();
        peer.write_all(OWNER.as_bytes()).unwrap();
        let completion = format!("{}\r\n", marker_response(&end));
        let split = completion.len() / 2;
        peer.write_all(&completion.as_bytes()[..split]).unwrap();
        thread::sleep(Duration::from_millis(30));
        peer.write_all(&completion.as_bytes()[split..]).unwrap();
    })
    .unwrap();
    assert!(response.starts_with(PRIMARY));
    assert!(response.contains(OWNER));
    assert!(response.contains("Player disconnected"));
    assert!(!response.contains("lsgm_ban_"));
    assert!(!response.contains("banner"));
}

#[test]
fn seven_days_telnet_never_treats_wrong_or_embedded_marker_as_completion() {
    let result = with_peer(Duration::from_secs(2), |mut peer| {
        let (_, end) = authenticate_and_read_command(&mut peer);
        peer.write_all(PRIMARY.as_bytes()).unwrap();
        peer.write_all(b"*** ERROR: unknown command 'lsgm_ban_end_wrong'\r\n")
            .unwrap();
        peer.write_all(format!("Player named {}\r\n", marker_response(&end)).as_bytes())
            .unwrap();
        peer.write_all(marker_response(&end).as_bytes()).unwrap(); // no terminating newline
    });
    assert!(
        result
            .unwrap_err()
            .contains("complete response could not be confirmed")
    );
}

#[test]
fn seven_days_telnet_missing_marker_exhausts_one_budget_without_resubmitting_ban() {
    let started = Instant::now();
    let result = with_peer(Duration::from_millis(160), |mut peer| {
        let (mut reader, _) = authenticate_and_read_command(&mut peer);
        peer.write_all(PRIMARY.as_bytes()).unwrap();
        let mut extra = String::new();
        assert_eq!(
            reader.read_line(&mut extra).unwrap(),
            0,
            "deadline closes connection without retry"
        );
        assert!(extra.is_empty());
    });
    assert!(result.unwrap_err().contains("total deadline"));
    assert!(started.elapsed() < Duration::from_secs(2));
}
