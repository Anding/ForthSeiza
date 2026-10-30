use crate::{Result, Worker};
use seiza_stacking::{
    ColorComposition, ColorCrop, ColorNormalization, ColorOptions, ForaxxOptions, LinearImage,
    LrgbLuminance, NarrowbandMatrix, NarrowbandMix, NarrowbandPalette, combine_lrgb,
    combine_narrowband, combine_narrowband_matrix, combine_rgb, combine_super_lrgb,
    combine_super_rgb,
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub mode: String,
    pub channels: BTreeMap<String, u32>,
    #[serde(default)]
    pub options: Options,
    pub directory: Option<PathBuf>,
    pub path: Option<PathBuf>,
    #[serde(default = "default_dimension")]
    pub max_dimension: usize,
}

fn default_dimension() -> usize {
    2048
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    normalization: Option<String>,
    black_percentile: Option<f32>,
    white_percentile: Option<f32>,
    normalization_samples: Option<usize>,
    crop: Option<String>,
    luminance_mode: Option<String>,
    luminance_weight: Option<f32>,
    foraxx_target_median: Option<f32>,
    foraxx_shadows_clip: Option<f32>,
    matrix: Option<Matrix>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Matrix {
    red: Mix,
    green: Mix,
    blue: Mix,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Mix {
    sii: f32,
    ha: f32,
    oiii: f32,
}

impl From<&Mix> for NarrowbandMix {
    fn from(m: &Mix) -> Self {
        Self {
            sii: m.sii,
            ha: m.ha,
            oiii: m.oiii,
        }
    }
}

impl Options {
    fn native(&self) -> Result<ColorOptions> {
        let normalization = match self.normalization.as_deref().unwrap_or("percentile") {
            "none" => {
                if self.black_percentile.is_some()
                    || self.white_percentile.is_some()
                    || self.normalization_samples.is_some()
                {
                    return Err("percentile options require percentile normalization".into());
                }
                ColorNormalization::None
            }
            "percentile" => ColorNormalization::Percentile {
                black_percentile: self.black_percentile.unwrap_or(0.001),
                white_percentile: self.white_percentile.unwrap_or(0.995),
                max_samples: self.normalization_samples.unwrap_or(1_000_000),
            },
            _ => return Err("normalization must be none or percentile".into()),
        };
        Ok(ColorOptions {
            normalization,
            crop: self
                .crop
                .as_deref()
                .unwrap_or("none")
                .parse::<ColorCrop>()
                .map_err(|e| e.to_string())?,
            ..ColorOptions::default()
        })
    }
}

pub fn compose(worker: &Worker, request: &Request) -> Result<(ColorComposition, String, u32)> {
    let mode = request.mode.to_ascii_uppercase();
    let matrix = request.options.matrix.as_ref().map(|m| NarrowbandMatrix {
        red: (&m.red).into(),
        green: (&m.green).into(),
        blue: (&m.blue).into(),
    });
    let palette = if mode != "RGB" && mode != "LRGB" && mode != "MATRIX" {
        Some(
            mode.parse::<NarrowbandPalette>()
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    let uses_sii = if let Some(palette) = palette {
        palette.requires_sii()
    } else if mode == "MATRIX" {
        let m = matrix.ok_or("matrix composition requires options.matrix")?;
        m.red.sii != 0.0 || m.green.sii != 0.0 || m.blue.sii != 0.0
    } else {
        false
    };
    let roles: &[&str] = match mode.as_str() {
        "RGB" => &["red", "green", "blue"],
        "LRGB" => &["luminance", "red", "green", "blue"],
        _ if uses_sii => &["ha", "oiii", "sii"],
        _ => &["ha", "oiii"],
    };
    if request.channels.len() != roles.len()
        || roles
            .iter()
            .any(|role| !request.channels.contains_key(*role))
    {
        return Err(format!(
            "{mode} requires exactly these channel roles: {}",
            roles.join(", ")
        ));
    }
    if mode != "MATRIX" && matrix.is_some() {
        return Err("options.matrix requires MATRIX mode".into());
    }
    if mode != "LRGB" && request.options.luminance_weight.is_some() {
        return Err("luminanceWeight requires LRGB".into());
    }
    if mode != "RGB" && mode != "LRGB" && request.options.luminance_mode.is_some() {
        return Err("luminanceMode requires RGB or LRGB".into());
    }
    if !palette.is_some_and(|p| p.transfer() == seiza_stacking::ColorTransfer::DisplayReferred)
        && (request.options.foraxx_target_median.is_some()
            || request.options.foraxx_shadows_clip.is_some())
    {
        return Err("Foraxx settings require a Foraxx palette".into());
    }
    let reference = request.channels[roles[0]];
    let grid = worker.stack(reference)?.grid;
    let mut images = BTreeMap::<&str, LinearImage>::new();
    for role in roles {
        let stack = worker.stack(request.channels[*role])?;
        if stack.grid != grid {
            return Err("colour channels must share an alignment grid".into());
        }
        let view = stack.native.view();
        if view.accepted_frames == 0 || view.channels != 1 {
            return Err(format!("{role} must contain accepted mono data"));
        }
        images.insert(
            role,
            stack
                .native
                .export_snapshot()
                .map_err(|e| e.to_string())?
                .image,
        );
    }
    let options = request.options.native()?;
    let image = |role: &str| &images[role];
    let super_luminance = match request
        .options
        .luminance_mode
        .as_deref()
        .unwrap_or("native")
    {
        "native" | "replace" => false,
        "super" => true,
        _ => return Err("luminanceMode must be native, replace or super".into()),
    };
    let (composition, label) = match mode.as_str() {
        "RGB" if super_luminance => (
            combine_super_rgb(image("red"), image("green"), image("blue"), &options),
            "SUPER-RGB".into(),
        ),
        "RGB" => (
            combine_rgb(image("red"), image("green"), image("blue"), &options),
            mode,
        ),
        "LRGB" => {
            let luminance = LrgbLuminance::new(super_luminance, request.options.luminance_weight)
                .map_err(|e| e.to_string())?;
            let composition = match luminance {
                LrgbLuminance::Super => combine_super_lrgb(
                    image("luminance"),
                    image("red"),
                    image("green"),
                    image("blue"),
                    &options,
                ),
                LrgbLuminance::Replace { luminance_weight } => combine_lrgb(
                    image("luminance"),
                    image("red"),
                    image("green"),
                    image("blue"),
                    luminance_weight,
                    &options,
                ),
            };
            (composition, luminance.label().into())
        }
        "MATRIX" => (
            combine_narrowband_matrix(
                image("ha"),
                image("oiii"),
                images.get("sii"),
                matrix.unwrap(),
                &options,
            ),
            NarrowbandMatrix::LABEL.into(),
        ),
        _ => {
            let palette = palette.unwrap();
            let defaults = ForaxxOptions::default();
            (
                combine_narrowband(
                    image("ha"),
                    image("oiii"),
                    images.get("sii"),
                    palette,
                    &options,
                    &ForaxxOptions {
                        target_median: request
                            .options
                            .foraxx_target_median
                            .unwrap_or(defaults.target_median),
                        shadows_clip: request
                            .options
                            .foraxx_shadows_clip
                            .unwrap_or(defaults.shadows_clip),
                    },
                ),
                palette.name().into(),
            )
        }
    };
    Ok((composition.map_err(|e| e.to_string())?, label, reference))
}
