use std::{
    io,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use lazy_allrounder_core::error::PortError;

const STOP_WAIT: Duration = Duration::from_millis(100);

pub fn spawn_capture(
    output_path: &str,
    rate: &str,
    channels: &str,
    format: &str,
) -> Result<Child, io::Error> {
    Command::new("pw-record")
        .args([
            "--rate",
            rate,
            "--channels",
            channels,
            "--format",
            format,
            output_path,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

pub fn process_is_alive(pid: u32) -> Result<bool, PortError> {
    let status = Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(map_io_error)?;

    Ok(status.success())
}

pub fn is_pw_record_process(pid: u32) -> Result<bool, PortError> {
    if !process_is_alive(pid)? {
        return Ok(false);
    }

    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "args="])
        .output()
        .map_err(map_io_error)?;

    if !output.status.success() {
        return Ok(false);
    }

    Ok(String::from_utf8_lossy(&output.stdout).contains("pw-record"))
}

pub fn stop_capture_process(pid: u32) -> Result<(), PortError> {
    send_signal(pid, "INT")?;

    for _ in 0..30 {
        if !process_is_alive(pid)? {
            return Ok(());
        }
        thread::sleep(STOP_WAIT);
    }

    send_signal(pid, "TERM")?;
    thread::sleep(Duration::from_millis(200));

    if process_is_alive(pid)? {
        send_signal(pid, "KILL")?;
    }

    Ok(())
}

pub fn map_spawn_error(error: io::Error) -> PortError {
    if error.kind() == io::ErrorKind::NotFound {
        return PortError::Other {
            message:
                "pw-record was not found; install PipeWire tools to use dictate capture on Linux"
                    .to_owned(),
        };
    }

    map_io_error(error)
}

pub fn map_io_error(error: io::Error) -> PortError {
    PortError::Other {
        message: error.to_string(),
    }
}

fn send_signal(pid: u32, signal: &str) -> Result<(), PortError> {
    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(map_io_error)?;

    if status.success() {
        return Ok(());
    }

    Err(PortError::Other {
        message: format!("failed to send SIG{signal} to pid {pid}"),
    })
}
