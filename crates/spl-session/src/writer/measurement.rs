//! `measurement.md`: front matter, description and summary.

use std::{
    fmt::Write as _,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use time::OffsetDateTime;

use super::attachments::WrittenAttachment;
use super::{SessionError, fixed, optional_fixed, utc_millis};
use crate::{
    BandLog, Calibration, FinalValues, LevelFinal, MeasurementBundle, MeasurementMetadata, Segment,
    band_column,
};

pub(crate) fn write_measurement_atomically(
    measurement_path: &Path,
    bundle: &MeasurementBundle,
    attachments: &[WrittenAttachment],
) -> Result<(), SessionError> {
    let temporary_path = measurement_path.with_extension(format!("md.{}.tmp", std::process::id()));
    let write_result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .map_err(|source| SessionError::Io {
                path: temporary_path.clone(),
                source,
            })?;
        file.write_all(render_measurement(bundle, attachments).as_bytes())
            .map_err(|source| SessionError::Io {
                path: temporary_path.clone(),
                source,
            })?;
        file.sync_all().map_err(|source| SessionError::Io {
            path: temporary_path.clone(),
            source,
        })?;
        fs::rename(&temporary_path, measurement_path).map_err(|source| SessionError::Io {
            path: measurement_path.to_owned(),
            source,
        })
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result
}

fn render_measurement(bundle: &MeasurementBundle, attachments: &[WrittenAttachment]) -> String {
    let metadata = &bundle.metadata;
    let (start_s, end_s) = summary_range(&bundle.segments);
    let mut markdown = String::new();
    markdown.push_str("---\nformat: splmeter-session\nversion: \"1.2\"\n");
    yaml_string(&mut markdown, "name", &metadata.name, 0);
    yaml_timestamp(&mut markdown, "start_utc", metadata.start_utc, 0);
    yaml_timestamp(&mut markdown, "end_utc", metadata.end_utc, 0);
    append(
        &mut markdown,
        format_args!(
            "duration_s: {}\n",
            fixed((metadata.end_utc - metadata.start_utc).as_seconds_f64(), 3)
        ),
    );
    yaml_string(&mut markdown, "software", &metadata.software, 0);
    append(
        &mut markdown,
        format_args!(
            "log_interval_s: {}\nreference_pressure_uPa: {}\n",
            fixed(metadata.log_interval_s, 3),
            metadata.reference_pressure_upa
        ),
    );
    render_device(&mut markdown, metadata);
    render_calibration(&mut markdown, &metadata.calibration);
    if let Some(author) = &metadata.author {
        yaml_string(&mut markdown, "author", author, 0);
    }
    render_tags(&mut markdown, &metadata.tags);
    if let Some(bands) = &bundle.bands {
        render_bands(&mut markdown, bands);
    }
    markdown.push_str("files:\n  log: log.csv\n  markers: markers.csv\n  segments: segments.csv\n");
    if bundle.bands.is_some() {
        markdown.push_str("  rta_log: rta_log.csv\n  rta_segments: rta_segments.csv\n");
    }
    render_attachments(&mut markdown, attachments);
    markdown.push_str("---\n\n# ");
    markdown.push_str(&metadata.name);
    markdown.push_str("\n\n## Description\n\n");
    markdown.push_str(&metadata.description);
    markdown.push_str("\n\n## Summary\n\n```yaml {#summary}\n");
    append(
        &mut markdown,
        format_args!(
            "range: {{ start_s: {}, end_s: {} }}\n",
            fixed(start_s, 3),
            fixed(end_s, 3)
        ),
    );
    render_summary_values(&mut markdown, bundle.summary.values);
    if let Some(bands) = &bundle.bands {
        markdown.push_str("rta_LZeq_db: ");
        yaml_list(
            &mut markdown,
            bands.summary_db.iter().map(|&level| yaml_level(level)),
        );
    }
    append(
        &mut markdown,
        format_args!("clip: {}\n```\n", bundle.summary.clipped),
    );
    markdown
}

fn render_device(markdown: &mut String, metadata: &MeasurementMetadata) {
    markdown.push_str("device:\n");
    yaml_string(markdown, "id", &metadata.device.id, 2);
    yaml_string(markdown, "name", &metadata.device.name, 2);
    yaml_string(markdown, "backend", &metadata.device.backend, 2);
    append(
        markdown,
        format_args!(
            "  channel: {}\n  sample_rate_hz: {}\n",
            metadata.device.channel, metadata.device.sample_rate_hz
        ),
    );
}

fn render_calibration(markdown: &mut String, calibration: &Calibration) {
    markdown.push_str("calibration:\n");
    append(
        markdown,
        format_args!("  offset_db: {}\n", fixed(calibration.offset_db, 2)),
    );
    append(
        markdown,
        format_args!("  type: {}\n", calibration.calibration_type.as_yaml()),
    );
    if let Some(level_db) = calibration.calibrator_level_db {
        append(
            markdown,
            format_args!("  calibrator_level_db: {}\n", fixed(level_db, 2)),
        );
    }
    if let Some(freq_hz) = calibration.calibrator_freq_hz {
        append(
            markdown,
            format_args!("  calibrator_freq_hz: {}\n", fixed(freq_hz, 2)),
        );
    }
    if let Some(timestamp) = calibration.calibrated_utc {
        yaml_timestamp(markdown, "calibrated_utc", timestamp, 2);
    }
}

fn render_bands(markdown: &mut String, bands: &BandLog) {
    append(
        markdown,
        format_args!(
            "rta:\n  quantity: LZeq\n  bands_per_octave: {}\n  bands_hz: ",
            bands.bands_per_octave
        ),
    );
    yaml_list(
        markdown,
        bands
            .bands_hz
            .iter()
            .map(|&nominal_hz| band_column(nominal_hz)["LZeq_".len()..].to_owned()),
    );
}

/// Writes a YAML flow sequence and ends the line.
fn yaml_list(markdown: &mut String, items: impl Iterator<Item = String>) {
    markdown.push('[');
    markdown.push_str(&items.collect::<Vec<_>>().join(", "));
    markdown.push_str("]\n");
}

fn yaml_level(level_db: Option<f64>) -> String {
    level_db.map_or_else(|| "null".to_owned(), |level_db| fixed(level_db, 2))
}

fn render_attachments(markdown: &mut String, attachments: &[WrittenAttachment]) {
    if attachments.is_empty() {
        return;
    }
    markdown.push_str("  attachments:\n");
    for attachment in attachments {
        markdown.push_str("    - ");
        yaml_string(markdown, "file", &attachment.file, 0);
        yaml_string(markdown, "title", &attachment.title, 6);
        yaml_string(markdown, "sha256", &attachment.sha256, 6);
    }
}

fn render_tags(markdown: &mut String, tags: &[String]) {
    if tags.is_empty() {
        markdown.push_str("tags: []\n");
        return;
    }
    markdown.push_str("tags:\n");
    for tag in tags {
        markdown.push_str("  - ");
        markdown.push_str(&yaml_quote(tag));
        markdown.push('\n');
    }
}

fn render_summary_values(markdown: &mut String, values: FinalValues) {
    summary_line(markdown, "A", values.a);
    summary_line(markdown, "C", values.c);
    summary_line(markdown, "Z", values.z);
}

fn summary_line(markdown: &mut String, name: &str, value: LevelFinal) {
    append(
        markdown,
        format_args!(
            "{name}: {{ Leq_db: {}, peak_db: {}, Fmax_db: {} }}\n",
            optional_fixed(value.leq_db, 2),
            optional_fixed(value.peak_db, 2),
            optional_fixed(value.fast_max_db, 2)
        ),
    );
}

fn summary_range(segments: &[Segment]) -> (f64, f64) {
    match (segments.first(), segments.last()) {
        (Some(first), Some(last)) => (first.start_s, last.end_s),
        _ => (0.0, 0.0),
    }
}

fn yaml_string(markdown: &mut String, key: &str, value: &str, indent: usize) {
    markdown.push_str(&" ".repeat(indent));
    markdown.push_str(key);
    markdown.push_str(": ");
    markdown.push_str(&yaml_quote(value));
    markdown.push('\n');
}

fn yaml_timestamp(markdown: &mut String, key: &str, value: OffsetDateTime, indent: usize) {
    yaml_string(markdown, key, &utc_millis(value), indent);
}

fn yaml_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn append(markdown: &mut String, arguments: std::fmt::Arguments<'_>) {
    // Writing to a String has no failure mode.
    markdown
        .write_fmt(arguments)
        .expect("writing a format argument into a String cannot fail");
}
