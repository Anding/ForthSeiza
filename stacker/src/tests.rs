use super::*;
use seiza_stacking::{LinearImage, write_processed_image_fits_f32};

fn fixture(path: &Path, shift: f32, background: f32) {
    let (width, height) = (192, 160);
    let mut data = vec![background; width * height];
    for (index, sample) in data.iter_mut().enumerate() {
        *sample += ((index * 19 + 7) % 101) as f32 * 0.04 - 2.0;
    }
    for star in 0..36 {
        let cx = 18.0 + ((star * 73 + 19) % 155) as f32 + shift;
        let cy = 18.0 + ((star * 47 + 13) % 124) as f32;
        let amplitude = 1200.0 + (star % 7) as f32 * 450.0;
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
}
