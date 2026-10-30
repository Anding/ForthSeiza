use super::*;
use seiza_stacking::{LinearImage, write_processed_image_fits_f32};

fn fixture(path: &Path, shift: f32, background: f32) {
    fixture_with_colour(path, shift, background, None);
}

fn fixture_with_colour(path: &Path, shift: f32, background: f32, band: Option<usize>) {
    let (width, height) = (192, 160);
    let mut data = vec![background; width * height];
    for (index, sample) in data.iter_mut().enumerate() {
        *sample += ((index * 19 + 7) % 101) as f32 * 0.04 - 2.0;
    }
    for star in 0..36 {
        let cx = 18.0 + ((star * 73 + 19) % 155) as f32 + shift;
        let cy = 18.0 + ((star * 47 + 13) % 124) as f32;
        let colour = band.map_or(1.0, |band| if star % 3 == band { 1.8 } else { 0.7 });
        let amplitude = (1200.0 + (star % 7) as f32 * 450.0) * colour;
        for y in (cy as usize - 6)..=(cy as usize + 6) {
            for x in (cx as usize - 6)..=(cx as usize + 6) {
                let radius = (x as f32 - cx).powi(2) + (y as f32 - cy).powi(2);
                data[y * width + x] += amplitude * (-radius / 3.2).exp();
            }
        }
    }
    let image = LinearImage::new(width, height, 1, data).unwrap();
    write_processed_image_fits_f32(path, &image, &[], &[]).unwrap();
}

#[test]
fn shared_grid_composes_native_modes_without_mutating_channels() {
    let dir = tempfile::tempdir().unwrap();
    let mut worker = Worker::default();
    let mut handles = Vec::new();
    for index in 0..4 {
        let path = dir.path().join(format!("channel-{index}.fits"));
        fixture(&path, index as f32 * 0.6, 100.0 + index as f32 * 10.0);
        let mut params = json!({"reference": path});
        if index != 0 {
            params["alignment_handle"] = json!(handles[0]);
        }
        let opened = worker.dispatch("open", params).unwrap();
        let handle = opened["handle"].as_u64().unwrap() as u32;
        if index == 0 {
            assert_eq!(opened["referenceIntegrated"], true);
        } else {
            assert_eq!(opened["acceptedFrames"], 0);
            assert_eq!(opened["referenceIntegrated"], false);
            assert_eq!(
                worker
                    .dispatch("add", json!({"handle": handle, "path": path}))
                    .unwrap()["accepted"],
                true
            );
        }
        handles.push(handle);
    }
    // Counts may differ; this must not alter channel registration or prevent composition.
    let extra = dir.path().join("extra.xisf");
    fixture(&extra, 0.2, 100.0);
    worker
        .dispatch("add", json!({"handle": handles[0], "path": extra}))
        .unwrap();
    let before: Vec<_> = handles
        .iter()
        .map(|h| worker.stack(*h).unwrap().native.export_snapshot().unwrap())
        .collect();
    for (mode, channels, options, transfer) in [
        (
            "RGB",
            json!({"red":handles[0],"green":handles[1],"blue":handles[2]}),
            json!({}),
            "LINEAR",
        ),
        (
            "RGB",
            json!({"red":handles[0],"green":handles[1],"blue":handles[2]}),
            json!({"luminanceMode":"super"}),
            "LINEAR",
        ),
        (
            "LRGB",
            json!({"luminance":handles[0],"red":handles[1],"green":handles[2],"blue":handles[3]}),
            json!({}),
            "LINEAR",
        ),
        (
            "LRGB",
            json!({"luminance":handles[0],"red":handles[1],"green":handles[2],"blue":handles[3]}),
            json!({"luminanceMode":"super"}),
            "LINEAR",
        ),
        (
            "SHO",
            json!({"ha":handles[0],"oiii":handles[1],"sii":handles[2]}),
            json!({}),
            "LINEAR",
        ),
        (
            "HOO",
            json!({"ha":handles[0],"oiii":handles[1]}),
            json!({}),
            "LINEAR",
        ),
        (
            "FORAXX-SHO",
            json!({"ha":handles[0],"oiii":handles[1],"sii":handles[2]}),
            json!({}),
            "DISPLAY",
        ),
        (
            "FORAXX-HOO",
            json!({"ha":handles[0],"oiii":handles[1]}),
            json!({}),
            "DISPLAY",
        ),
        (
            "MATRIX",
            json!({"ha":handles[0],"oiii":handles[1],"sii":handles[2]}),
            json!({"matrix":{"red":{"sii":1},"green":{"ha":1},"blue":{"oiii":1}}}),
            "LINEAR",
        ),
    ] {
        for extension in ["fits", "xisf"] {
            let path = dir.path().join(format!("colour-{mode}.{extension}"));
            let result = worker
                .dispatch(
                    "colour-save",
                    json!({"mode":mode,"channels":channels,"options":options,"path":path}),
                )
                .unwrap();
            assert_eq!(result["transfer"], transfer);
            let saved = seiza_image_io::open(&path).unwrap();
            assert_eq!(saved.channels, 3);
            let text = std::fs::read(&path).unwrap();
            assert!(String::from_utf8_lossy(&text).contains(transfer));
        }
        let directory = dir.path().join(format!("preview-{mode}"));
        let result = worker.dispatch("colour-preview", json!({
            "mode":mode,"channels":channels,"options":options,"directory":directory,"max_dimension":96,
        })).unwrap();
        assert_eq!(
            (result["width"].as_u64(), result["height"].as_u64()),
            (Some(96), Some(80))
        );
        assert_eq!(result["transfer"], transfer);
        let retained = std::fs::read(directory.join("latest.dat")).unwrap();
        assert!(
            worker
                .dispatch(
                    "colour-preview",
                    json!({
                        "mode":mode,"channels":channels,"directory":directory,"max_dimension":0,
                    })
                )
                .is_err()
        );
        assert_eq!(
            std::fs::read(directory.join("latest.dat")).unwrap(),
            retained
        );
    }
    // Measure star positions rather than comparing intensities changed by interpolation.
    let request = json!({"mode":"RGB","channels":{"red":handles[0],"green":handles[1],"blue":handles[2]},
        "options":{"normalization":"none"}});
    let (rgb, _, _) = colour::compose(&worker, &decode(request).unwrap()).unwrap();
    let data = &rgb.image.data;
    for channel in 1..3 {
        let centroid = |channel: usize, cx: usize, cy: usize| {
            let samples: Vec<_> = (cy - 4..=cy + 4)
                .flat_map(|y| {
                    (cx - 4..=cx + 4).map(move |x| {
                        (x as f64, y as f64, data[(y * 192 + x) * 3 + channel] as f64)
                    })
                })
                .collect();
            let background = samples.iter().map(|p| p.2).fold(f64::INFINITY, f64::min);
            let (mut sum, mut sx, mut sy) = (0.0, 0.0, 0.0);
            for (x, y, value) in samples {
                let weight = (value - background).max(0.0);
                sum += weight;
                sx += x * weight;
                sy += y * weight;
            }
            (sx / sum, sy / sum)
        };
        let mut squared = 0.0;
        for star in 0..36 {
            let cx = 18 + (star * 73 + 19) % 155;
            let cy = 18 + (star * 47 + 13) % 124;
            let a = centroid(0, cx, cy);
            let b = centroid(channel, cx, cy);
            squared += (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2);
        }
        let rms = (squared / 36.0).sqrt();
        assert!(
            rms < 0.15,
            "registered channel {channel} centroid RMS: {rms}"
        );
    }
    for (handle, before) in handles.iter().zip(before) {
        let after = worker
            .stack(*handle)
            .unwrap()
            .native
            .export_snapshot()
            .unwrap();
        assert_eq!(after.accepted_frames, before.accepted_frames);
        assert!(
            after
                .image
                .data
                .iter()
                .zip(before.image.data)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
    }
}

#[test]
fn colour_requires_live_populated_handles_on_one_grid_and_protects_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reference.fits");
    fixture(&path, 0.0, 100.0);
    let mut worker = Worker::default();
    let root = worker.dispatch("open", json!({"reference":path})).unwrap()["handle"]
        .as_u64()
        .unwrap();
    let empty = worker
        .dispatch("open", json!({"reference":path,"alignment_handle":root}))
        .unwrap()["handle"]
        .as_u64()
        .unwrap();
    let other = worker.dispatch("open", json!({"reference":path})).unwrap()["handle"]
        .as_u64()
        .unwrap();
    let base = json!({"mode":"RGB","channels":{"red":root,"green":root,"blue":root},"directory":dir.path().join("preview")});
    for invalid in [empty, other, 9999] {
        let mut request = base.clone();
        request["channels"]["blue"] = json!(invalid);
        assert!(worker.dispatch("colour-preview", request).is_err());
    }
    for channels in [
        json!({"red":root,"blue":root}),
        json!({"red":root,"green":root,"blue":root,"ha":root}),
    ] {
        let mut request = base.clone();
        request["channels"] = channels;
        assert!(worker.dispatch("colour-preview", request).is_err());
    }
    let mut request = base.clone();
    request.as_object_mut().unwrap().remove("directory");
    request["path"] = json!(path);
    assert!(
        worker
            .dispatch("colour-save", request)
            .unwrap_err()
            .contains("overwrite")
    );
    let mut params =
        json!({"reference":path,"alignment_handle":root,"options":StackOptions::default()});
    params["options"]["bin_factor"] = json!(2);
    assert!(
        worker
            .dispatch("open", params)
            .unwrap_err()
            .contains("bin_factor")
    );
    worker.dispatch("close", json!({"handle":root})).unwrap();
    assert!(worker.dispatch("colour-preview", base).is_err());
    assert!(
        worker
            .dispatch("open", json!({"reference":path,"handle":root}))
            .is_err()
    );
}

#[test]
fn foraxx_preview_preserves_native_display_samples_without_a_second_stretch() {
    let dir = tempfile::tempdir().unwrap();
    let image =
        LinearImage::new(2, 1, 3, vec![0.1, 0.2, 0.3, f32::NAN, f32::NAN, f32::NAN]).unwrap();
    let composition = seiza_stacking::ColorComposition {
        image,
        transfer: seiza_stacking::ColorTransfer::DisplayReferred,
        region: seiza_stacking::ReferenceRegion {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
        },
        crop: None,
    };
    let request =
        decode(json!({"mode":"FORAXX-HOO","channels":{},"directory":dir.path(),"max_dimension":2}))
            .unwrap();
    let result = preview::publish_colour(&composition, "FORAXX-HOO", &request).unwrap();
    let bmp = dir
        .path()
        .join(result["generation"].as_str().unwrap())
        .join("stack.bmp");
    let rgb = image::open(bmp).unwrap().to_rgb8();
    assert_eq!(rgb.get_pixel(0, 0).0, [26, 51, 77]);
    assert_eq!(rgb.get_pixel(1, 0).0, [0, 0, 0]);
}

#[test]
fn native_stack_accepts_shifted_fits_and_xisf_without_readding_reference() {
    let dir = tempfile::tempdir().unwrap();
    let reference = dir.path().join("reference.fits");
    let second = dir.path().join("second.xisf");
    fixture(&reference, 0.0, 100.0);
    fixture(&second, 1.0, 120.0);
    let mut worker = Worker::default();
    let opened = worker
        .dispatch("open", json!({"reference": reference}))
        .unwrap();
    let handle = opened["handle"].as_u64().unwrap();
    assert_eq!(opened["acceptedFrames"], 1);
    assert_eq!(opened["referenceIntegrated"], true);
    let added = worker
        .dispatch("add", json!({"handle": handle, "path": second}))
        .unwrap();
    assert_eq!(added["accepted"], true, "{added}");
    assert_eq!(added["acceptedFrames"], 2);
    assert!(
        worker
            .dispatch("add", json!({"handle": handle, "path": reference}))
            .is_err()
    );
    assert_eq!(
        worker.dispatch("state", json!({"handle": handle})).unwrap()["acceptedFrames"],
        2
    );
    for extension in ["fits", "xisf"] {
        let output = dir.path().join(format!("linear.{extension}"));
        worker
            .dispatch("save", json!({"handle": handle, "path": output}))
            .unwrap();
        let saved = seiza_image_io::open(&output).unwrap();
        assert!(saved.samples.iter().any(|sample| *sample > 1000.0));
    }
    assert!(
        worker
            .dispatch("save", json!({"handle": handle, "path": reference}))
            .is_err()
    );
    let publication = dir.path().join("preview");
    let preview = worker
        .dispatch(
            "preview",
            json!({
                "handle": handle, "directory": publication, "max_dimension": 96,
            }),
        )
        .unwrap();
    assert_eq!(preview["width"], 96);
    assert_eq!(preview["height"], 80);
    let manifest = std::fs::read_to_string(publication.join("latest.dat")).unwrap();
    assert!(
        manifest
            .lines()
            .any(|line| line.starts_with("time-stamp: \""))
    );
    let encoded = manifest.lines().next().unwrap().split_once(": ").unwrap().1;
    let bmp: String = serde_json::from_str(encoded).unwrap();
    let displayed = image::open(publication.join(bmp)).unwrap();
    assert_eq!((displayed.width(), displayed.height()), (96, 80));
    worker
        .dispatch(
            "preview",
            json!({"handle": handle, "directory": publication}),
        )
        .unwrap();
    let retained = std::fs::read(publication.join("latest.dat")).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let generations = std::fs::read_dir(&publication).unwrap().count();
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(publication.join("latest.dat"))
            .unwrap();
        assert!(
            worker
                .dispatch(
                    "preview",
                    json!({
                        "handle": handle, "directory": publication,
                    })
                )
                .is_err()
        );
        drop(locked);
        assert_eq!(
            std::fs::read_dir(&publication).unwrap().count(),
            generations
        );
    }
    assert!(
        worker
            .dispatch(
                "preview",
                json!({
                    "handle": handle, "directory": publication, "max_dimension": 0,
                })
            )
            .is_err()
    );
    assert_eq!(
        std::fs::read(publication.join("latest.dat")).unwrap(),
        retained
    );
    worker.dispatch("close", json!({"handle": handle})).unwrap();
    assert!(worker.dispatch("state", json!({"handle": handle})).is_err());
}

#[test]
fn native_bias_calibration_is_used_and_the_master_is_protected() {
    let dir = tempfile::tempdir().unwrap();
    let reference = dir.path().join("reference.fits");
    let bias = dir.path().join("bias.xisf");
    fixture(&reference, 0.0, 100.0);
    let master = LinearImage::new(192, 160, 1, vec![10.0; 192 * 160]).unwrap();
    write_processed_image_fits_f32(&bias, &master, &[], &[]).unwrap();
    let mut worker = Worker::default();
    let opened = worker
        .dispatch("open", json!({"reference": reference, "bias": bias}))
        .unwrap();
    let handle = opened["handle"].as_u64().unwrap() as u32;
    let source = seiza_image_io::open(&reference).unwrap();
    let mean = worker.stack(handle).unwrap().native.view().mean[0];
    assert!((mean - (source.samples[0] - 10.0)).abs() < 0.0001);
    assert!(
        worker
            .dispatch("save", json!({"handle": handle, "path": bias}))
            .is_err()
    );
    let channel = dir.path().join("channel.fits");
    let channel_bias = dir.path().join("channel-bias.xisf");
    fixture(&channel, 0.0, 200.0);
    let master = LinearImage::new(192, 160, 1, vec![30.0; 192 * 160]).unwrap();
    write_processed_image_fits_f32(&channel_bias, &master, &[], &[]).unwrap();
    let opened = worker
        .dispatch(
            "open",
            json!({
                "reference":channel, "alignment_handle":handle, "bias":channel_bias,
            }),
        )
        .unwrap();
    let other = opened["handle"].as_u64().unwrap() as u32;
    assert_eq!(
        worker
            .dispatch("add", json!({"handle":other,"path":channel}))
            .unwrap()["accepted"],
        true
    );
    let source = seiza_image_io::open(&channel).unwrap();
    let index = 10 * 192 + 10;
    let mean = worker.stack(other).unwrap().native.view().mean[index];
    assert!((mean - (source.samples[index] - 30.0)).abs() < 0.01);
    for path in [&bias, &channel_bias] {
        assert!(worker.dispatch("colour-save", json!({
            "mode":"RGB", "channels":{"red":handle,"green":other,"blue":other},"path":path,
        })).unwrap_err().contains("overwrite"));
    }
}

#[test]
fn rejected_frame_is_not_an_operational_error() {
    let dir = tempfile::tempdir().unwrap();
    let reference = dir.path().join("reference.fits");
    let second = dir.path().join("drifting.fits");
    fixture(&reference, 0.0, 100.0);
    fixture(&second, 2.0, 100.0);
    let mut worker = Worker::default();
    let opened = worker
        .dispatch(
            "open",
            json!({
                "reference": reference,
                "options": {"acceptance": {"minimum_overlap_fraction": 0.99999}},
            }),
        )
        .unwrap();
    let handle = opened["handle"].as_u64().unwrap();
    let rejected = worker
        .dispatch("add", json!({"handle": handle, "path": second}))
        .unwrap();
    assert_eq!(rejected["accepted"], false, "{rejected}");
    assert_eq!(rejected["acceptedFrames"], 1);
    assert_eq!(rejected["rejectedFrames"], 1);
    assert!(rejected["reason"].as_str().unwrap().contains("overlap"));
    assert!(
        worker
            .dispatch(
                "add",
                json!({"handle": handle, "path": dir.path().join("missing.fits")})
            )
            .is_err()
    );
}

#[test]
fn protocol_preserves_ids_and_recovers_from_known_request_errors() {
    let requests = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":1}}\n",
        "not json\n",
        "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"state\",\"params\":{\"handle\":999}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"shutdown\"}\n",
    );
    let mut bytes = Vec::new();
    serve(std::io::Cursor::new(requests), &mut bytes).unwrap();
    let responses: Vec<Value> = String::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 4);
    assert_eq!(responses[0]["result"]["protocolVersion"], 1);
    assert_eq!(responses[1]["error"]["code"], -32700);
    assert_eq!(responses[2]["id"], 7);
    assert!(responses[2].get("error").is_some());
    assert_eq!(responses[3]["id"], 9);
}

#[test]
#[ignore = "writes fixtures for the offline 32-bit Forth bridge test"]
fn export_offline_fixtures() {
    let directory = PathBuf::from(std::env::var("SEIZA_STACKER_FIXTURES").unwrap());
    std::fs::create_dir_all(&directory).unwrap();
    fixture(&directory.join("reference.fits"), 0.0, 100.0);
    fixture(&directory.join("second.xisf"), 1.0, 120.0);
    fixture(&directory.join("drifting.fits"), 2.0, 100.0);
    for (band, name) in ["red", "green", "blue"].iter().enumerate() {
        for round in 1..=2 {
            fixture_with_colour(
                &directory.join(format!("{name}-{round}.fits")),
                band as f32 * 0.6 + (round - 1) as f32 * 0.2,
                100.0 + band as f32 * 10.0,
                Some(band),
            );
        }
    }
}
