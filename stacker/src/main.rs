mod preview;

use seiza_stacking::{
    FrameDisposition, LiveStacker, StackOptions, paths_refer_to_same_file,
    write_stack_export_fits_f32,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

const MAX_LINE: u64 = 1024 * 1024;
type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    reference: PathBuf,
    #[serde(default)]
    handle: Option<u32>,
    #[serde(default)]
    bias: Option<PathBuf>,
    #[serde(default)]
    dark: Option<PathBuf>,
    #[serde(default)]
    flat: Option<PathBuf>,
    #[serde(default)]
    dark_exposure_seconds: Option<f64>,
    #[serde(default)]
    options: StackOptions,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Handle {
    handle: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRequest {
    handle: u32,
    path: PathBuf,
}

struct Stack {
    native: LiveStacker,
    reference: PathBuf,
}

#[derive(Default)]
struct Worker {
    stacks: BTreeMap<u32, Stack>,
    next_handle: u32,
}

impl Worker {
    fn stack(&self, handle: u32) -> Result<&Stack> {
        self.stacks
            .get(&handle)
            .ok_or_else(|| format!("invalid stack handle {handle}"))
    }

    fn dispatch(&mut self, method: &str, params: Value) -> Result<Value> {
        match method {
            "initialize" => {
                if params["protocolVersion"] != 1 {
                    return Err("unsupported protocol version".into());
                }
                Ok(json!({"protocolVersion": 1, "stackingVersion": seiza_stacking::VERSION}))
            }
            "open" => {
                let request: Open = decode(params)?;
                astronomy_path(&request.reference)?;
                let handle = request.handle.unwrap_or(self.next_handle + 1);
                if handle == 0 || handle > i32::MAX as u32 || self.stacks.contains_key(&handle) {
                    return Err("invalid or already open handle".into());
                }
                let native = LiveStacker::open_fits(
                    &request.reference,
                    request.bias.as_deref(),
                    request.dark.as_deref(),
                    request.flat.as_deref(),
                    request.dark_exposure_seconds,
                    request.options,
                )
                .map_err(|e| e.to_string())?;
                let stack = Stack {
                    native,
                    reference: request.reference,
                };
                let response = state_json(handle, &stack);
                self.stacks.insert(handle, stack);
                self.next_handle = self.next_handle.max(handle);
                Ok(response)
            }
            "add" => {
                let request: FileRequest = decode(params)?;
                astronomy_path(&request.path)?;
                let stack = self
                    .stacks
                    .get_mut(&request.handle)
                    .ok_or_else(|| format!("invalid stack handle {}", request.handle))?;
                let disposition = stack
                    .native
                    .push_fits(&request.path)
                    .map_err(|e| e.to_string())?;
                let mut result = state_json(request.handle, stack);
                match disposition {
                    FrameDisposition::Accepted(d) => {
                        result["accepted"] = json!(true);
                        result["diagnostics"] = json!({
                            "matchedStars": d.matched_stars,
                            "registrationRmsPixels": d.registration_rms_pixels,
                            "registrationDriftPixels": d.registration_drift_pixels,
                            "transform": d.transform,
                            "overlapFraction": d.overlap_fraction,
                            "integratedFraction": d.integrated_fraction,
                            "acceptedSamples": d.accepted_samples,
                            "rejectedSamples": d.rejected_samples,
                            "normalizationMeanGain": d.normalization_mean_gain,
                            "normalizationMeanOffset": d.normalization_mean_offset,
                        });
                    }
                    FrameDisposition::Rejected(reason) => {
                        result["accepted"] = json!(false);
                        result["reason"] = json!(reason.to_string());
                    }
                }
                Ok(result)
            }
            "state" => {
                let request: Handle = decode(params)?;
                Ok(state_json(request.handle, self.stack(request.handle)?))
            }
            "preview" => {
                let request: preview::Request = decode(params)?;
                let stack = self.stack(request.handle)?;
                preview::publish(&stack.native, &request)
            }
            "save" => {
                let request: FileRequest = decode(params)?;
                astronomy_path(&request.path)?;
                if self
                    .stacks
                    .values()
                    .flat_map(|s| s.native.protected_paths())
                    .any(|input| paths_refer_to_same_file(&input, &request.path))
                {
                    return Err(
                        "stack output must not overwrite an input or calibration master".into(),
                    );
                }
                let stack = self.stack(request.handle)?;
                let snapshot = stack.native.export_snapshot().map_err(|e| e.to_string())?;
                write_stack_export_fits_f32(
                    &request.path,
                    &snapshot,
                    stack.native.reference_headers(),
                )
                .map_err(|e| e.to_string())?;
                Ok(json!({"path": request.path, "acceptedFrames": snapshot.accepted_frames}))
            }
            "close" => {
                let request: Handle = decode(params)?;
                self.stacks
                    .remove(&request.handle)
                    .ok_or_else(|| format!("invalid stack handle {}", request.handle))?;
                Ok(json!({"closed": request.handle}))
            }
            "shutdown" => {
                self.stacks.clear();
                Ok(json!({"shutdown": true}))
            }
            _ => Err(format!("unknown method {method}")),
        }
    }
}

fn state_json(handle: u32, stack: &Stack) -> Value {
    let view = stack.native.view();
    json!({
        "handle": handle, "acceptedFrames": view.accepted_frames,
        "rejectedFrames": view.rejected_frames, "width": view.width,
        "height": view.height, "channels": view.channels, "reference": stack.reference,
        "referenceIntegrated": stack.native.reference_is_integrated(),
    })
}

fn decode<T: serde::de::DeserializeOwned>(params: Value) -> Result<T> {
    serde_json::from_value(params).map_err(|e| e.to_string())
}

fn astronomy_path(path: &Path) -> Result<()> {
    match seiza_image_io::Format::from_path(path) {
        Some(format) if format.is_astronomy() => Ok(()),
        _ => Err("expected a completed linear FITS or XISF path".into()),
    }
}

fn serve(mut input: impl BufRead, mut output: impl Write) -> Result<()> {
    let mut worker = Worker::default();
    loop {
        let mut line = String::new();
        let count = std::io::Read::take(&mut input, MAX_LINE + 1)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(());
        }
        if count as u64 > MAX_LINE {
            return Err("request exceeds 1 MiB".into());
        }
        let request: std::result::Result<Value, _> = serde_json::from_str(&line);
        let mut shutdown = false;
        let response = match request {
            Ok(request) => {
                let id = request.get("id").cloned().unwrap_or(Value::Null);
                let result = if request["jsonrpc"] != "2.0" {
                    Err("expected JSON-RPC 2.0".into())
                } else if let Some(method) = request["method"].as_str() {
                    shutdown = method == "shutdown";
                    worker.dispatch(method, request.get("params").cloned().unwrap_or(json!({})))
                } else {
                    Err("missing method".into())
                };
                match result {
                    Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                    Err(error) => json!({"jsonrpc": "2.0", "id": id,
                        "error": {"code": -32000, "message": error}}),
                }
            }
            Err(error) => json!({"jsonrpc": "2.0", "id": null,
                "error": {"code": -32700, "message": error.to_string()}}),
        };
        serde_json::to_writer(&mut output, &response).map_err(|e| e.to_string())?;
        output
            .write_all(b"\n")
            .and_then(|_| output.flush())
            .map_err(|e| e.to_string())?;
        if shutdown {
            return Ok(());
        }
    }
}

fn main() {
    if let Err(error) = serve(std::io::stdin().lock(), std::io::stdout().lock()) {
        eprintln!("seiza-stacker: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests;
