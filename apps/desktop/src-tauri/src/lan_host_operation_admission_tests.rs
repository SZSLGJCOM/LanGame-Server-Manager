use super::*;
use std::sync::Condvar;
use std::sync::mpsc::{self, Receiver};

struct OperationHost {
    address: std::net::SocketAddr,
    shutdown: Arc<AtomicBool>,
    release: Arc<(Mutex<bool>, Condvar)>,
    slots: Arc<Semaphore>,
    started: Arc<AtomicUsize>,
    completed: Arc<AtomicUsize>,
    entered: Receiver<()>,
    handled: Receiver<()>,
    done: Receiver<Result<LanHostRunSummary, String>>,
    server: Option<thread::JoinHandle<()>>,
}

impl OperationHost {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = Arc::clone(&shutdown);
        let shutdown_check: ShutdownCheck =
            Arc::new(move || worker_shutdown.load(Ordering::SeqCst));
        let handler_shutdown = Arc::clone(&shutdown_check);
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let handler_release = Arc::clone(&release);
        let slots = Arc::new(Semaphore::new(LAN_HOST_MAX_OWNED_REQUESTS));
        let handler_slots = Arc::clone(&slots);
        let started = Arc::new(AtomicUsize::new(0));
        let handler_started = Arc::clone(&started);
        let completed = Arc::new(AtomicUsize::new(0));
        let handler_completed = Arc::clone(&completed);
        let (entered_tx, entered) = mpsc::channel();
        let (handled_tx, handled) = mpsc::channel();
        let handler: ConnectionHandler = Arc::new(move |mut stream| {
            stream
                .set_read_timeout(Some(Duration::from_millis(20)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_millis(20)))
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            let request = read_request_head(&mut stream, &handler_shutdown, deadline)?;
            let result = handle_api_request(
                &mut stream,
                request,
                Arc::new(access()),
                &handler_shutdown,
                deadline,
                &handler_slots,
                |command, _| match command {
                    "install_module_game"
                    | "start_instance_process"
                    | "create_instance_backup"
                    | "restore_instance_backup" => {
                        handler_started.fetch_add(1, Ordering::SeqCst);
                        entered_tx.send(()).unwrap();
                        let (lock, changed) = &*handler_release;
                        let (released, _) = changed
                            .wait_timeout_while(
                                lock.lock().unwrap(),
                                Duration::from_secs(5),
                                |released| !*released,
                            )
                            .unwrap();
                        if !*released {
                            return Err("Test did not release the accepted operation".into());
                        }
                        handler_completed.fetch_add(1, Ordering::SeqCst);
                        Ok(json!({ "completed": true }))
                    }
                    "cancel_installation_job" | "read_background_jobs" => {
                        Ok(json!({ "command": command }))
                    }
                    _ => panic!("unexpected test command: {command}"),
                },
            );
            let _ = handled_tx.send(());
            result
        });
        let (done_tx, done) = mpsc::channel();
        let server = thread::spawn(move || {
            let result = run_lan_listener(
                listener,
                handler,
                shutdown_check,
                LAN_HOST_WORKER_COUNT,
                LAN_HOST_QUEUE_CAPACITY,
                Duration::from_millis(2),
            );
            let _ = done_tx.send(result);
        });
        Self {
            address,
            shutdown,
            release,
            slots,
            started,
            completed,
            entered,
            handled,
            done,
            server: Some(server),
        }
    }

    fn request(&self, command: &str) -> TcpStream {
        let mut client = TcpStream::connect(self.address).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let body = json!({ "command": command }).to_string();
        let request = format!(
            "POST /__langame/api HTTP/1.1\r\nHost: localhost\r\nX-LanGame-Token: {}\r\nContent-Length: {}\r\n\r\n{body}",
            token_text(7),
            body.len(),
        );
        client.write_all(request.as_bytes()).unwrap();
        client
    }

    fn wait_for_start(&self) {
        self.entered
            .recv_timeout(Duration::from_secs(2))
            .expect("accepted operation must begin without releasing another operation");
    }

    fn wait_for_handler(&self) {
        self.handled
            .recv_timeout(Duration::from_secs(2))
            .expect("request handler must finish and release its permit");
    }

    fn release_operations(&self) {
        let (lock, changed) = &*self.release;
        *lock.lock().unwrap() = true;
        changed.notify_all();
    }

    fn finish(mut self) -> LanHostRunSummary {
        self.release_operations();
        self.shutdown.store(true, Ordering::SeqCst);
        let summary = self
            .done
            .recv_timeout(Duration::from_secs(3))
            .expect("all LAN workers must join after their accepted operations finish")
            .unwrap();
        self.server.take().unwrap().join().unwrap();
        summary
    }
}

impl Drop for OperationHost {
    fn drop(&mut self) {
        self.release_operations();
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(server) = self.server.take()
            && self.done.recv_timeout(Duration::from_secs(6)).is_ok()
        {
            let _ = server.join();
        }
    }
}

fn read_api_response(mut client: TcpStream) -> (u16, Value) {
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap())
}

#[test]
fn owned_operations_leave_lan_workers_available_for_control_requests() {
    let host = OperationHost::start();
    assert!(host.slots.available_permits() < LAN_HOST_WORKER_COUNT);
    let first = host.request("install_module_game");
    host.wait_for_start();
    let second = host.request("start_instance_process");
    host.wait_for_start();

    let (status, response) = read_api_response(host.request("install_module_game"));
    assert_eq!(status, 503);
    assert_eq!(response["ok"], false);
    assert!(
        response["error"]
            .as_str()
            .unwrap()
            .contains("no operation was started")
    );
    host.wait_for_handler();
    for command in ["cancel_installation_job", "read_background_jobs"] {
        let (status, response) = read_api_response(host.request(command));
        assert_eq!(status, 200);
        assert_eq!(response["value"]["command"], command);
        host.wait_for_handler();
    }
    assert_eq!(host.started.load(Ordering::SeqCst), 2);
    assert_eq!(host.completed.load(Ordering::SeqCst), 0);

    host.release_operations();
    for request in [first, second] {
        let (status, response) = read_api_response(request);
        assert_eq!(status, 200);
        assert_eq!(response["value"]["completed"], true);
        host.wait_for_handler();
    }
    assert_eq!(host.completed.load(Ordering::SeqCst), 2);
    assert_eq!(host.slots.available_permits(), LAN_HOST_MAX_OWNED_REQUESTS);
    assert_eq!(host.finish().accepted_connections, 5);
}

#[test]
fn owned_operation_slots_outlive_disconnect_and_return_after_completion() {
    let host = OperationHost::start();
    for command in ["create_instance_backup", "restore_instance_backup"] {
        let client = host.request(command);
        host.wait_for_start();
        client.shutdown(Shutdown::Both).unwrap();
    }

    let (status, _) = read_api_response(host.request("install_module_game"));
    assert_eq!(status, 503, "disconnect must not release accepted work");
    host.wait_for_handler();
    assert_eq!(host.slots.available_permits(), 0);
    assert_eq!(host.started.load(Ordering::SeqCst), 2);
    assert_eq!(host.completed.load(Ordering::SeqCst), 0);

    host.release_operations();
    host.wait_for_handler();
    host.wait_for_handler();
    assert_eq!(host.completed.load(Ordering::SeqCst), 2);
    assert_eq!(host.slots.available_permits(), LAN_HOST_MAX_OWNED_REQUESTS);
    let (status, response) = read_api_response(host.request("install_module_game"));
    assert_eq!(status, 200);
    assert_eq!(response["value"]["completed"], true);
    host.wait_for_handler();
    assert_eq!(host.started.load(Ordering::SeqCst), 3);
    assert_eq!(host.completed.load(Ordering::SeqCst), 3);
    assert_eq!(host.slots.available_permits(), LAN_HOST_MAX_OWNED_REQUESTS);
    assert_eq!(host.finish().accepted_connections, 4);
}
