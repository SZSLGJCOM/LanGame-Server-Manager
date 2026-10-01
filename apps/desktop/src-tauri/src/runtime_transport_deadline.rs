use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

pub(super) const TRANSPORT_BUDGET: Duration = Duration::from_secs(8);
const IO_TIMEOUT: Duration = Duration::from_secs(3);

/// A synchronous worker retains one deadline across connection, authentication,
/// framing, and command response. Every partial read/write reduces the remaining
/// socket timeout; a trickling peer cannot renew the operation's lifetime.
pub(crate) struct DeadlineTcpStream {
    stream: TcpStream,
    deadline: Instant,
    idle_read_timeout: Duration,
    deadline_exhausted: bool,
}

impl DeadlineTcpStream {
    pub(super) fn connect(endpoint: &str, idle_read_timeout: Duration) -> io::Result<Self> {
        Self::connect_with_budget(endpoint, idle_read_timeout, TRANSPORT_BUDGET)
    }

    pub(crate) fn connect_with_budget(
        endpoint: &str,
        idle_read_timeout: Duration,
        budget: Duration,
    ) -> io::Result<Self> {
        let deadline = Instant::now() + budget;
        // Runtime endpoints are assembled from an instance bind IP and port.
        // DNS resolution has no synchronous cancellation boundary on Windows.
        let address = endpoint.parse::<SocketAddr>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "runtime endpoint must be an IP address and port",
            )
        })?;
        let stream = TcpStream::connect_timeout(&address, remaining(deadline)?.min(IO_TIMEOUT))?;
        let stream = Self {
            stream,
            deadline,
            idle_read_timeout,
            deadline_exhausted: false,
        };
        stream.check_deadline()?;
        Ok(stream)
    }

    pub(super) fn check_deadline(&self) -> io::Result<()> {
        if self.deadline_exhausted {
            return Err(deadline_error());
        }
        remaining(self.deadline).map(|_| ())
    }

    pub(super) fn pause(&self, duration: Duration) -> io::Result<()> {
        std::thread::sleep(duration.min(remaining(self.deadline)?));
        self.check_deadline()
    }

    pub(super) fn shutdown(&self) -> io::Result<()> {
        self.stream.shutdown(Shutdown::Both)
    }
}

impl Read for DeadlineTcpStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.check_deadline()?;
        let time_remaining = remaining(self.deadline)?;
        self.stream
            .set_read_timeout(Some(time_remaining.min(self.idle_read_timeout)))?;
        let result = self.stream.read(buffer);
        // Socket timeout precision differs by platform. A timeout bounded by the
        // total budget must not become Telnet's successful idle-response boundary.
        if time_remaining <= self.idle_read_timeout && is_timeout(&result) {
            self.deadline_exhausted = true;
        }
        self.check_deadline()?;
        result
    }
}

impl Write for DeadlineTcpStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.check_deadline()?;
        let time_remaining = remaining(self.deadline)?;
        self.stream
            .set_write_timeout(Some(time_remaining.min(IO_TIMEOUT)))?;
        let result = self.stream.write(buffer);
        if time_remaining <= IO_TIMEOUT && is_timeout(&result) {
            self.deadline_exhausted = true;
        }
        self.check_deadline()?;
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        self.check_deadline()?;
        self.stream.flush()
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(deadline_error)
}

fn deadline_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "runtime command total deadline exceeded",
    )
}

fn is_timeout<T>(result: &io::Result<T>) -> bool {
    matches!(result, Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut))
}

#[cfg(test)]
#[path = "runtime_transport_deadline_tests.rs"]
mod tests;
