use crate::Result;
use seiza_stacking::{ColorComposition, ColorTransfer, LiveStacker, StackView};
use seiza_stretch::{
    ImageShape, SampleDomain, SampleNormalization, StretchConfig, StretchParams, StretchStack,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub handle: u32,
    pub directory: PathBuf,
    #[serde(default = "default_dimension")]
    pub max_dimension: usize,
}

fn default_dimension() -> usize {
    2048
}

pub fn publish(stack: &LiveStacker, request: &Request) -> Result<Value> {
    let view = stack.view();
    let name = format!(
        "Live stack: {} accepted, {} rejected",
        view.accepted_frames, view.rejected_frames
    );
    publish_view(view, request, ColorTransfer::LinearLight, &name)
}

pub fn publish_colour(
    composition: &ColorComposition,
    label: &str,
    request: &crate::colour::Request,
) -> Result<Value> {
    if request.path.is_some() {
        return Err("colour-preview takes directory, not path".into());
    }
    let image = &composition.image;
    let view = StackView {
        width: image.width,
        height: image.height,
        channels: image.channels,
        mean: &image.data,
        coverage: &[],
        rejected_samples: &[],
        accepted_frames: 0,
        rejected_frames: 0,
    };
    let mut result = publish_view(
        view,
        &Request {
            handle: 0,
            directory: request
                .directory
                .clone()
                .ok_or("colour-preview requires directory")?,
            max_dimension: request.max_dimension,
        },
        composition.transfer,
        &format!("Live colour: {label}"),
    )?;
    result["composition"] = json!(label);
    result["transfer"] = json!(composition.transfer.fits_name());
    Ok(result)
}

fn publish_view(
    view: StackView<'_>,
    request: &Request,
    transfer: ColorTransfer,
    name: &str,
) -> Result<Value> {
    if !(1..=8192).contains(&request.max_dimension) {
        return Err("preview max_dimension must be between 1 and 8192".into());
    }
    let scale = (request.max_dimension as f64 / view.width.max(view.height) as f64).min(1.0);
    let width = ((view.width as f64 * scale).round() as usize).max(1);
    let height = ((view.height as f64 * scale).round() as usize).max(1);
    let channels = view.channels;
    let mut samples = vec![f32::NAN; width * height * channels];
    // Downsample only for display. Uncovered samples never enter native stretch analysis.
    for y in 0..height {
        let y0 = y * view.height / height;
        let y1 = ((y + 1) * view.height / height).max(y0 + 1);
        for x in 0..width {
            let x0 = x * view.width / width;
            let x1 = ((x + 1) * view.width / width).max(x0 + 1);
            for c in 0..channels {
                let mut sum = 0.0f64;
                let mut count = 0usize;
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let index = (sy * view.width + sx) * channels + c;
                        if view.coverage.get(index).is_none_or(|count| *count > 0)
                            && view.mean[index].is_finite()
                        {
                            sum += view.mean[index] as f64;
                            count += 1;
                        }
                    }
                }
                if count > 0 {
                    samples[(y * width + x) * channels + c] = (sum / count as f64) as f32;
                }
            }
        }
    }
    let pixels = if transfer == ColorTransfer::DisplayReferred {
        samples
            .iter()
            .map(|sample| (sample.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect()
    } else {
        let domain = SampleDomain::PhysicalLinear {
            normalization: SampleNormalization::default(),
        }
        .resolve(&samples, channels)
        .map_err(|e| e.to_string())?;
        domain
            .apply_in_place(&mut samples, channels)
            .map_err(|e| e.to_string())?;
        let stretched =
            StretchStack::single(StretchConfig::auto_mtf(StretchParams::default(), 200_000))
                .apply_u8_image(&samples, ImageShape::new(width, height, channels))
                .map_err(|e| e.to_string())?;
        stretched.data
    };
    let mut rgb = Vec::with_capacity(width * height * 3);
    for (linear, pixel) in samples
        .chunks_exact(channels)
        .zip(pixels.chunks_exact(channels))
    {
        if !linear.iter().all(|sample| sample.is_finite()) {
            rgb.extend_from_slice(&[0, 0, 0]);
        } else if channels == 1 {
            rgb.extend_from_slice(&[pixel[0]; 3]);
        } else {
            rgb.extend_from_slice(pixel);
        }
    }
    fs::create_dir_all(&request.directory).map_err(|e| e.to_string())?;
    let generation = uuid::Uuid::new_v4().to_string();
    let directory = request.directory.join(&generation);
    fs::create_dir(&directory).map_err(|e| e.to_string())?;
    let publication = (|| {
        let image = directory.join("stack.bmp");
        let mut file = File::create(&image).map_err(|e| e.to_string())?;
        image::codecs::bmp::BmpEncoder::new(&mut file)
            .encode(
                &rgb,
                width as u32,
                height as u32,
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        let relative = Path::new(&generation).join("stack.bmp");
        let manifest = format!(
            "image filepath: {}\nstretched image filepath: {}\nimage name: {}\ntime-stamp: {}\n",
            json!(relative),
            json!(relative),
            json!(name),
            json!(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        );
        atomic_manifest(&request.directory, manifest.as_bytes())?;
        Ok(json!({
            "manifest": request.directory.join("latest.dat"),
            "generation": generation, "width": width, "height": height,
            "acceptedFrames": view.accepted_frames, "rejectedFrames": view.rejected_frames,
        }))
    })();
    if publication.is_err() {
        let _ = fs::remove_file(directory.join("stack.bmp"));
        let _ = fs::remove_dir(&directory);
    }
    publication
}

fn atomic_manifest(directory: &Path, data: &[u8]) -> Result<()> {
    let temporary = directory.join(format!(".latest-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = File::create_new(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        replace(&temporary, &directory.join("latest.dat"))
    })()
    .map_err(|e: std::io::Error| e.to_string());
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
