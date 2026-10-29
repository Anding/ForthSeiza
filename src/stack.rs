//! Independent, non-replaying transport for the 64-bit stacking worker.

use serde_json::{Value, json};
use std::ffi::c_char;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::Duration;

struct Transport {
    child: Child,
    input: Option<mpsc::Sender<Vec<u8>>>,
    output: mpsc::Receiver<Result<String, String>>,
    timeout: Duration,
    next_id: u64,
    alive: bool,
}

impl Transport {
    fn start(executable: &str, timeout_seconds: u32) -> Result<Self, String> {
        if !(1..=3600).contains(&timeout_seconds) {
            return Err("stack timeout must be between 1 and 3600 seconds".into());
        }
        let mut command = Command::new(executable);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        Self::start_command(command, timeout_seconds)
    }

    fn start_command(mut command: Command, timeout_seconds: u32) -> Result<Self, String> {
        let mut child = command
            .spawn()
            .map_err(|e| format!("cannot start stack worker: {e}"))?;
        let mut writer = BufWriter::new(child.stdin.take().ok_or("stack stdin unavailable")?);
        let stdout = child.stdout.take().ok_or("stack stdout unavailable")?;
        let (input, requests) = mpsc::channel::<Vec<u8>>();
        let (responses, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            for request in requests {
                // Both pipe directions are off the calling thread, so the timeout
                // also covers a child that stops reading a large options request.
                let result = (|| {
                    writer
                        .write_all(&request)
                        .and_then(|_| writer.flush())
                        .map_err(|error| format!("cannot write stack request: {error}"))?;
                    let mut line = String::new();
                    match (&mut reader).take(1024 * 1024 + 1).read_line(&mut line) {
                        Ok(0) => Err("stack worker closed stdout".into()),
                        Ok(size) if size > 1024 * 1024 => Err("stack reply exceeds 1 MiB".into()),
                        Ok(_) => Ok(line),
                        Err(error) => Err(format!("cannot read stack reply: {error}")),
                    }
                })();
                let ended = result.is_err();
                if responses.send(result).is_err() || ended {
                    break;
                }
            }
        });
        let mut transport = Self {
            child,
            input: Some(input),
            output,
            timeout: Duration::from_secs(timeout_seconds as u64),
            next_id: 0,
            alive: true,
        };
        let initialized = transport.request(
            "initialize",
            json!({
                "protocolVersion": 1, "clientName": "ForthSeiza",
            }),
        )?;
        if initialized["protocolVersion"] != 1 {
            return Err("unsupported stack protocol".into());
        }
        Ok(transport)
    }

    fn invalidate(&mut self, reason: impl std::fmt::Display) -> String {
        self.alive = false;
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        format!(
            "stack transport lost: {reason}; outcome unknown, worker stopped, all handles invalid; no request replayed"
        )
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        if !self.alive {
            return Err("stack worker is stopped; all previous handles are invalid".into());
        }
        self.next_id += 1;
        let request =
            json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params});
        let mut encoded = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        encoded.push(b'\n');
        if let Err(error) = self
            .input
            .as_ref()
            .ok_or("stack input closed")?
            .send(encoded)
        {
            return Err(self.invalidate(error));
        }
        let line = match self.output.recv_timeout(self.timeout) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => return Err(self.invalidate(error)),
            Err(error) => return Err(self.invalidate(error)),
        };
        let response: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => return Err(self.invalidate(error)),
        };
        if response["jsonrpc"] != "2.0"
            || response["id"] != self.next_id
            || response.get("result").is_some() == response.get("error").is_some()
        {
            return Err(self.invalidate("invalid response envelope or request ID"));
        }
        if let Some(error) = response.get("error") {
            return Err(error["message"]
                .as_str()
                .unwrap_or("stack request failed")
                .to_owned());
        }
        Ok(response["result"].clone())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
struct Bridge {
    transport: Option<Transport>,
    // Tokens are never reused in this DLL lifetime, even after an explicit restart.
    next_handle: u32,
    handles: std::collections::BTreeSet<u32>,
    error: String,
    reply: String,
}

static STACK: OnceLock<Mutex<Bridge>> = OnceLock::new();

fn bridge() -> &'static Mutex<Bridge> {
    STACK.get_or_init(|| Mutex::new(Bridge::default()))
}

impl Bridge {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let transport = self
            .transport
            .as_mut()
            .ok_or("stack worker has not been started")?;
        if method != "open" {
            let handle = params["handle"].as_u64().unwrap_or(0) as u32;
            if !self.handles.contains(&handle) {
                return Err("invalid or stale stack handle".into());
            }
        }
        let result = transport.request(method, params)?;
        self.reply = result.to_string();
        Ok(result)
    }
}

fn call(operation: impl FnOnce(&mut Bridge) -> Result<i32, String>) -> i32 {
    let mut bridge = match bridge().lock() {
        Ok(value) => value,
        Err(_) => return -1,
    };
    bridge.error.clear();
    bridge.reply.clear();
    match operation(&mut bridge) {
        Ok(result) => result,
        Err(error) => {
            bridge.error = error;
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn SeizaStackStart(
    executable: *const c_char,
    length: usize,
    timeout_seconds: u32,
) -> i32 {
    call(|bridge| {
        let executable = unsafe { super::input_string(executable, length, "stack executable")? };
        // Starting is explicit, never a recovery/retry operation.
        bridge.transport.take();
        bridge.handles.clear();
        bridge.transport = Some(Transport::start(&executable, timeout_seconds)?);
        Ok(1)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn SeizaStackStop() -> i32 {
    call(|bridge| {
        bridge.transport.take();
        bridge.handles.clear();
        Ok(1)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn SeizaStackOpen(
    reference: *const c_char,
    reference_length: usize,
    config: *const c_char,
    config_length: usize,
) -> i32 {
    call(|bridge| {
        let reference =
            unsafe { super::input_string(reference, reference_length, "reference path")? };
        let config =
            unsafe { super::optional_input_string(config, config_length, "stack config")? };
        let mut params: Value = if let Some(path) = config {
            let bytes =
                std::fs::read(path).map_err(|e| format!("cannot read stack config: {e}"))?;
            if bytes.len() > 64 * 1024 {
                return Err("stack config exceeds 64 KiB".into());
            }
            serde_json::from_slice(&bytes).map_err(|e| format!("invalid stack config: {e}"))?
        } else {
            json!({})
        };
        if !params.is_object()
            || params.get("reference").is_some()
            || params.get("handle").is_some()
        {
            return Err("stack config must be an object without reference or handle".into());
        }
        bridge.next_handle = bridge
            .next_handle
            .checked_add(1)
            .filter(|handle| *handle <= i32::MAX as u32)
            .ok_or("stack handles exhausted")?;
        let handle = bridge.next_handle;
        params["reference"] = json!(reference);
        params["handle"] = json!(handle);
        let response = bridge.request("open", params)?;
        if response["handle"] != handle {
            return Err("unexpected stack handle".into());
        }
        bridge.handles.insert(handle);
        Ok(handle as i32)
    })
}

unsafe fn path_request(
    handle: u32,
    path: *const c_char,
    length: usize,
    method: &str,
    key: &str,
) -> i32 {
    call(|bridge| {
        let path = unsafe { super::input_string(path, length, "stack path")? };
        let result = bridge.request(method, json!({"handle": handle, key: path}))?;
        if method == "add" {
            result["accepted"]
                .as_bool()
                .map(i32::from)
                .ok_or("missing stack disposition".into())
        } else {
            Ok(1)
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn SeizaStackAdd(handle: u32, path: *const c_char, length: usize) -> i32 {
    unsafe { path_request(handle, path, length, "add", "path") }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn SeizaStackPreview(handle: u32, path: *const c_char, length: usize) -> i32 {
    unsafe { path_request(handle, path, length, "preview", "directory") }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn SeizaStackSave(handle: u32, path: *const c_char, length: usize) -> i32 {
    unsafe { path_request(handle, path, length, "save", "path") }
}

#[unsafe(no_mangle)]
pub extern "C" fn SeizaStackState(handle: u32) -> i32 {
    call(|bridge| {
        bridge.request("state", json!({"handle": handle}))?;
        Ok(1)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn SeizaStackClose(handle: u32) -> i32 {
    call(|bridge| {
        bridge.request("close", json!({"handle": handle}))?;
        bridge.handles.remove(&handle);
        Ok(1)
    })
}

unsafe fn copy_text(output: *mut c_char, capacity: usize, error: bool) -> usize {
    if output.is_null() || capacity == 0 {
        return 0;
    }
    let bridge = match bridge().lock() {
        Ok(value) => value,
        Err(_) => return 0,
    };
    let text = if error { &bridge.error } else { &bridge.reply };
    let count = text.len().min(capacity - 1);
    unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr(), output.cast::<u8>(), count);
        *output.add(count) = 0;
    }
    count
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn SeizaStackLastError(output: *mut c_char, capacity: usize) -> usize {
    unsafe { copy_text(output, capacity, true) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn SeizaStackLastReply(output: *mut c_char, capacity: usize) -> usize {
    unsafe { copy_text(output, capacity, false) }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn fake_worker(second_response: &str) -> Transport {
        let script = format!(
            "$null = [Console]::ReadLine(); \
             [Console]::WriteLine('{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{\"protocolVersion\":1}}}}'); \
             {second_response}"
        );
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &script,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        Transport::start_command(command, 1).unwrap()
    }

    #[test]
    fn mutations_are_never_replayed_after_a_timeout_or_wrong_response_id() {
        for (reply, params) in [
            (
                "$null = [Console]::ReadLine(); Start-Sleep -Seconds 10",
                json!({}),
            ),
            (
                "$null = [Console]::ReadLine(); [Console]::WriteLine('{\"jsonrpc\":\"2.0\",\"id\":999,\"result\":{}}')",
                json!({}),
            ),
            (
                "Start-Sleep -Seconds 10",
                json!({"largeOptions": "x".repeat(262_144)}),
            ),
        ] {
            let mut worker = fake_worker(reply);
            let error = worker.request("add", params).unwrap_err();
            assert!(error.contains("outcome unknown"));
            assert!(error.contains("no request replayed"));
            assert!(!worker.alive);
            assert!(worker.child.try_wait().unwrap().is_some());
            assert!(
                worker
                    .request("add", json!({}))
                    .unwrap_err()
                    .contains("stopped")
            );
            assert_eq!(worker.next_id, 2);
        }
    }

    #[test]
    fn known_rejection_errors_do_not_restart_the_worker() {
        let mut worker = fake_worker(
            "$null = [Console]::ReadLine(); [Console]::WriteLine('{\"jsonrpc\":\"2.0\",\"id\":2,\"error\":{\"message\":\"missing file\"}}'); Start-Sleep -Seconds 10",
        );
        assert_eq!(
            worker.request("add", json!({})).unwrap_err(),
            "missing file"
        );
        assert!(worker.alive);
        assert!(worker.child.try_wait().unwrap().is_none());
    }
}
