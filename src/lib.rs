//! Stable C boundary between VFX Forth and Seiza's persistent JSON-RPC worker.

#![allow(non_snake_case)]

use serde_json::{Value, json};
use std::ffi::c_char;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::slice;
use std::sync::{Mutex, OnceLock};

const SOLVED: i32 = 1;
const UNSOLVED: i32 = 0;
const ERROR: i32 = -1;

static BRIDGE: OnceLock<Mutex<Option<Bridge>>> = OnceLock::new();
static LAST_ERROR: OnceLock<Mutex<String>> = OnceLock::new();

#[derive(Clone)]
struct WorkerConfig {
    executable: String,
    catalog: String,
    index: Option<String>,
}

struct Bridge {
    config: WorkerConfig,
    worker: Worker,
}

struct Worker {
    child: Child,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
    request_id: u64,
}

impl Worker {
    fn start(config: &WorkerConfig) -> Result<Self, String> {
        let mut command = Command::new(&config.executable);
        command
            .arg("worker")
            .arg("--data")
            .arg(&config.catalog)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(index) = &config.index {
            command.arg("--index").arg(index);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("cannot start Seiza worker: {error}"))?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| "cannot open Seiza worker stdin".to_string())?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| "cannot open Seiza worker stdout".to_string())?;
        let mut worker = Self {
            child,
            input: BufWriter::new(input),
            output: BufReader::new(output),
            request_id: 1,
        };
        worker.request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientName": "ForthSeiza",
                "clientVersion": env!("CARGO_PKG_VERSION")
            }),
        )?;
        Ok(worker)
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.request_id;
        self.request_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        serde_json::to_writer(&mut self.input, &request)
            .map_err(|error| format!("cannot encode Seiza request: {error}"))?;
        self.input
            .write_all(b"\n")
            .and_then(|_| self.input.flush())
            .map_err(|error| format!("cannot write Seiza request: {error}"))?;

        let mut line = String::new();
        let count = self
            .output
            .read_line(&mut line)
            .map_err(|error| format!("cannot read Seiza response: {error}"))?;
        if count == 0 {
            return Err("Seiza worker closed stdout".to_string());
        }
        let response: Value = serde_json::from_str(&line)
            .map_err(|error| format!("invalid Seiza response: {error}"))?;
        if response.get("id") != Some(&json!(id)) {
            return Err("Seiza response ID does not match request".to_string());
        }
        if let Some(error) = response.get("error") {
            let code = error.get("code").and_then(Value::as_i64);
            if code == Some(-32010) {
                return Ok(Value::Null);
            }
            return Err(format!("Seiza worker error: {error}"));
        }
        response
            .get("result")
            .cloned()
            .ok_or_else(|| "Seiza response has neither result nor error".to_string())
    }

    fn stop(mut self) {
        let _ = self.request("shutdown", json!({}));
        let _ = self.child.wait();
    }
}

impl Bridge {
    fn start(config: WorkerConfig) -> Result<Self, String> {
        Ok(Self {
            worker: Worker::start(&config)?,
            config,
        })
    }

    fn solve(&mut self, request: SolveRequest<'_>) -> Result<bool, String> {
        match self.worker.request("solve", request.json()) {
            Ok(Value::Null) => Ok(false),
            Ok(result) => {
                write_wcs_sidecar(Path::new(request.image_path), &result)?;
                Ok(true)
            }
            Err(first_error) => {
                self.worker = Worker::start(&self.config)
                    .map_err(|error| format!("{first_error}; restart failed: {error}"))?;
                match self.worker.request("solve", request.json()) {
                    Ok(Value::Null) => Ok(false),
                    Ok(result) => {
                        write_wcs_sidecar(Path::new(request.image_path), &result)?;
                        Ok(true)
                    }
                    Err(error) => Err(format!("{first_error}; retry failed: {error}")),
                }
            }
        }
    }
}

struct SolveRequest<'a> {
    image_path: &'a str,
    ra_deg: f64,
    dec_deg: f64,
    pixel_size_um: f64,
    focal_length_mm: f64,
    radius_deg: f64,
    scale_tolerance: f64,
}

impl SolveRequest<'_> {
    fn json(&self) -> Value {
        json!({
            "imagePath": self.image_path,
            "mode": "hinted",
            "hint": {
                "centerRaDeg": self.ra_deg,
                "centerDecDeg": self.dec_deg,
                "radiusDeg": self.radius_deg,
                "scaleArcsecPerPixel": 206.265 * self.pixel_size_um / self.focal_length_mm,
                "scaleTolerance": self.scale_tolerance
            }
        })
    }
}

fn write_wcs_sidecar(image_path: &Path, result: &Value) -> Result<(), String> {
    let wcs = result
        .get("wcs")
        .ok_or_else(|| "Seiza result has no WCS".to_string())?;
    let projection = wcs
        .get("projection")
        .and_then(Value::as_str)
        .ok_or_else(|| "Seiza WCS has no projection".to_string())?;
    let crval = pair(wcs, "crval")?;
    let crpix = pair(wcs, "crpix")?;
    let cd = matrix(wcs, "cd")?;
    let output_path = image_path.with_extension("wcs");
    let file = File::create(&output_path)
        .map_err(|error| format!("cannot create {}: {error}", output_path.display()))?;
    let mut writer = BufWriter::new(file);
    for (key, value) in [
        ("CTYPE1", format!("'RA---{projection}'")),
        ("CTYPE2", format!("'DEC--{projection}'")),
        ("CUNIT1", "'deg'".to_string()),
        ("CUNIT2", "'deg'".to_string()),
        ("RADESYS", "'ICRS'".to_string()),
        ("CRVAL1", fits_float(crval[0])),
        ("CRVAL2", fits_float(crval[1])),
        ("CRPIX1", fits_float(crpix[0])),
        ("CRPIX2", fits_float(crpix[1])),
        ("CD1_1", fits_float(cd[0][0])),
        ("CD1_2", fits_float(cd[0][1])),
        ("CD2_1", fits_float(cd[1][0])),
        ("CD2_2", fits_float(cd[1][1])),
    ] {
        write_card(&mut writer, key, &value)?;
    }
    write_card(&mut writer, "END", "")?;
    writer
        .flush()
        .map_err(|error| format!("cannot flush {}: {error}", output_path.display()))
}

fn pair(value: &Value, name: &str) -> Result<[f64; 2], String> {
    let values = value
        .get(name)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Seiza WCS has no {name} pair"))?;
    if values.len() != 2 {
        return Err(format!("Seiza WCS {name} must have two values"));
    }
    Ok([
        finite_number(&values[0], name)?,
        finite_number(&values[1], name)?,
    ])
}

fn matrix(value: &Value, name: &str) -> Result<[[f64; 2]; 2], String> {
    let values = value
        .get(name)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Seiza WCS has no {name} matrix"))?;
    if values.len() != 2 {
        return Err(format!("Seiza WCS {name} must have two rows"));
    }
    let row = |index: usize| {
        let row = values[index]
            .as_array()
            .ok_or_else(|| format!("Seiza WCS {name} row is not an array"))?;
        if row.len() != 2 {
            return Err(format!("Seiza WCS {name} row must have two values"));
        }
        Ok([finite_number(&row[0], name)?, finite_number(&row[1], name)?])
    };
    Ok([row(0)?, row(1)?])
}

fn finite_number(value: &Value, name: &str) -> Result<f64, String> {
    let value = value
        .as_f64()
        .ok_or_else(|| format!("Seiza WCS {name} contains a non-number"))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("Seiza WCS {name} contains a non-finite number"))
    }
}

fn fits_float(value: f64) -> String {
    format!("{value:.12E}")
}

fn write_card(writer: &mut BufWriter<File>, key: &str, value: &str) -> Result<(), String> {
    let card = if key == "END" {
        format!("{key:<80}")
    } else {
        format!("{key:<8}= {value:<70}")
    };
    writer
        .write_all(card.as_bytes())
        .and_then(|_| writer.write_all(b"\r\n"))
        .map_err(|error| format!("cannot write WCS card: {error}"))
}

unsafe fn input_string(
    pointer: *const c_char,
    length: usize,
    name: &str,
) -> Result<String, String> {
    if pointer.is_null() {
        return Err(format!("{name} pointer is null"));
    }
    let bytes = unsafe { slice::from_raw_parts(pointer.cast::<u8>(), length) };
    let value = std::str::from_utf8(bytes)
        .map_err(|error| format!("{name} is not UTF-8: {error}"))?
        .trim();
    if value.is_empty() {
        return Err(format!("{name} is empty"));
    }
    Ok(value.to_string())
}

unsafe fn optional_input_string(
    pointer: *const c_char,
    length: usize,
    name: &str,
) -> Result<Option<String>, String> {
    if length == 0 {
        return Ok(None);
    }
    if pointer.is_null() {
        return Err(format!("{name} pointer is null"));
    }
    let bytes = unsafe { slice::from_raw_parts(pointer.cast::<u8>(), length) };
    let value = std::str::from_utf8(bytes)
        .map_err(|error| format!("{name} is not UTF-8: {error}"))?
        .trim();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value.to_string()))
    }
}

fn parse_number(value: &str, name: &str) -> Result<f64, String> {
    let value = value
        .parse::<f64>()
        .map_err(|error| format!("{name} is not a number: {error}"))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("{name} is not finite"))
    }
}

fn bridge() -> &'static Mutex<Option<Bridge>> {
    BRIDGE.get_or_init(|| Mutex::new(None))
}

fn last_error() -> &'static Mutex<String> {
    LAST_ERROR.get_or_init(|| Mutex::new(String::new()))
}

fn set_error(error: impl Into<String>) {
    *last_error().lock().expect("error lock poisoned") = error.into();
}

fn clear_error() {
    last_error().lock().expect("error lock poisoned").clear();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn SeizaBridgeStart(
    executable: *const c_char,
    executable_length: usize,
    catalog: *const c_char,
    catalog_length: usize,
    index: *const c_char,
    index_length: usize,
) -> i32 {
    let result: Result<(), String> = (|| {
        let config = WorkerConfig {
            executable: unsafe { input_string(executable, executable_length, "Seiza executable")? },
            catalog: unsafe { input_string(catalog, catalog_length, "Seiza catalog")? },
            index: unsafe { optional_input_string(index, index_length, "Seiza index")? },
        };
        let mut bridge = bridge()
            .lock()
            .map_err(|_| "bridge lock poisoned".to_string())?;
        if let Some(existing) = bridge.take() {
            existing.worker.stop();
        }
        *bridge = Some(Bridge::start(config)?);
        Ok(())
    })();
    match result {
        Ok(()) => {
            clear_error();
            SOLVED
        }
        Err(error) => {
            set_error(error);
            ERROR
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn SeizaBridgeSolve(
    image_path: *const c_char,
    image_path_length: usize,
    ra_deg: *const c_char,
    ra_deg_length: usize,
    dec_deg: *const c_char,
    dec_deg_length: usize,
    pixel_size_um: *const c_char,
    pixel_size_um_length: usize,
    focal_length_mm: *const c_char,
    focal_length_mm_length: usize,
    radius_deg: *const c_char,
    radius_deg_length: usize,
    scale_tolerance: *const c_char,
    scale_tolerance_length: usize,
) -> i32 {
    let result: Result<bool, String> = (|| {
        let image_path = unsafe { input_string(image_path, image_path_length, "image path")? };
        let ra_deg = parse_number(&unsafe { input_string(ra_deg, ra_deg_length, "RA")? }, "RA")?;
        let dec_deg = parse_number(
            &unsafe { input_string(dec_deg, dec_deg_length, "Dec")? },
            "Dec",
        )?;
        let pixel_size_um = parse_number(
            &unsafe { input_string(pixel_size_um, pixel_size_um_length, "XPIXSZ")? },
            "XPIXSZ",
        )?;
        let focal_length_mm = parse_number(
            &unsafe { input_string(focal_length_mm, focal_length_mm_length, "FOCALLEN")? },
            "FOCALLEN",
        )?;
        let radius_deg = parse_number(
            &unsafe { input_string(radius_deg, radius_deg_length, "Seiza radius")? },
            "Seiza radius",
        )?;
        let scale_tolerance = parse_number(
            &unsafe {
                input_string(
                    scale_tolerance,
                    scale_tolerance_length,
                    "Seiza scale tolerance",
                )?
            },
            "Seiza scale tolerance",
        )?;
        if !(0.0..=360.0).contains(&ra_deg)
            || !(-90.0..=90.0).contains(&dec_deg)
            || pixel_size_um <= 0.0
            || focal_length_mm <= 0.0
            || radius_deg <= 0.0
            || !(0.0..=1.0).contains(&scale_tolerance)
        {
            return Err("invalid Seiza hinted-solve values".to_string());
        }
        let mut bridge = bridge()
            .lock()
            .map_err(|_| "bridge lock poisoned".to_string())?;
        let bridge = bridge
            .as_mut()
            .ok_or_else(|| "Seiza bridge has not been started".to_string())?;
        bridge.solve(SolveRequest {
            image_path: &image_path,
            ra_deg,
            dec_deg,
            pixel_size_um,
            focal_length_mm,
            radius_deg,
            scale_tolerance,
        })
    })();
    match result {
        Ok(true) => {
            clear_error();
            SOLVED
        }
        Ok(false) => {
            clear_error();
            UNSOLVED
        }
        Err(error) => {
            set_error(error);
            ERROR
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn SeizaBridgeStop() -> i32 {
    let result: Result<(), String> = (|| {
        let mut bridge = bridge()
            .lock()
            .map_err(|_| "bridge lock poisoned".to_string())?;
        if let Some(existing) = bridge.take() {
            existing.worker.stop();
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            clear_error();
            SOLVED
        }
        Err(error) => {
            set_error(error);
            ERROR
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn SeizaBridgeLastError(output: *mut c_char, capacity: usize) -> i32 {
    if output.is_null() || capacity == 0 {
        return ERROR;
    }
    let error = last_error().lock().expect("error lock poisoned");
    let bytes = error.as_bytes();
    let count = bytes.len().min(capacity - 1);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output.cast::<u8>(), count);
        *output.add(count) = 0;
    }
    count as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solve_request_calculates_pixel_scale() {
        let request = SolveRequest {
            image_path: r"E:\images\solve.fits",
            ra_deg: 328.43,
            dec_deg: 47.27,
            pixel_size_um: 3.76,
            focal_length_mm: 2350.0,
            radius_deg: 2.0,
            scale_tolerance: 0.2,
        };
        let scale = request.json()["hint"]["scaleArcsecPerPixel"]
            .as_f64()
            .unwrap();
        assert!((scale - 0.330024).abs() < 0.000001);
    }

    #[test]
    fn wcs_sidecar_is_fits_card_aligned() {
        let directory = std::env::temp_dir().join("forthseiza-wcs-test");
        std::fs::create_dir_all(&directory).unwrap();
        let image = directory.join("solve.fits");
        let result = json!({
            "wcs": {
                "projection": "TAN",
                "crval": [328.4308, 47.2731],
                "crpix": [3124.5, 2088.5],
                "cd": [[-0.00009, 0.0], [0.0, 0.00009]]
            }
        });
        write_wcs_sidecar(&image, &result).unwrap();
        let bytes = std::fs::read(image.with_extension("wcs")).unwrap();
        assert!(bytes.starts_with(b"CTYPE1  = 'RA---TAN'"));
        assert_eq!(bytes.len() % 82, 0);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn blank_optional_index_is_not_a_configuration_error() {
        let index = b" ";
        assert_eq!(
            unsafe {
                optional_input_string(index.as_ptr().cast(), index.len(), "Seiza index").unwrap()
            },
            None
        );
    }
}
