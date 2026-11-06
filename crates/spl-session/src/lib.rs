#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Session records, journals, and portable measurement bundles.

mod bands;
mod journal;
mod model;
mod range;
mod writer;

#[cfg(test)]
mod test_support;

pub use bands::{BandLog, BandRow, BandSegment, aggregate_band_segments, band_column};
pub use journal::{JournalRows, JournalWriter, RECOVERY_FILE};
pub use model::{
    AggregateError, Attachment, Calibration, CalibrationType, Device, FinalValues, LevelFinal,
    LogRecord, Marker, MarkerKind, MeasurementBundle, MeasurementMetadata, Segment,
    SegmentAggregate, aggregate_segments,
};
pub use range::{RangeError, segments_in_range};
pub use writer::{BundleWriter, SessionError};
