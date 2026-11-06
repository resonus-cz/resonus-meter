//! Markers whose label is entered after the click.
//!
//! The click reserves the timestamp and snapshots the open segment, so audio
//! processed while the label dialog is open belongs to the following segment.

use spl_session::MarkerKind;

use super::{ClosedSegment, SegmentLevels, SessionRecorder};

#[derive(Debug, Clone)]
pub(super) struct ReservedMarker {
    elapsed_s: f64,
    sample_count: u64,
    a: SegmentLevels,
    c: SegmentLevels,
    z: SegmentLevels,
    clipped: bool,
    following_a: SegmentLevels,
    following_c: SegmentLevels,
    following_z: SegmentLevels,
    following_clipped: bool,
}

impl ReservedMarker {
    pub(super) const fn elapsed_s(&self) -> f64 {
        self.elapsed_s
    }

    /// Adds a sample recorded after the click to the following segment.
    pub(super) fn process_following(
        &mut self,
        weighted: [f64; 3],
        fast_dbfs: [f64; 3],
        clipped: bool,
    ) {
        self.following_a.process(weighted[0], fast_dbfs[0]);
        self.following_c.process(weighted[1], fast_dbfs[1]);
        self.following_z.process(weighted[2], fast_dbfs[2]);
        self.following_clipped |= clipped;
    }
}

impl SessionRecorder {
    pub(crate) fn reserve_marker_timestamp(&mut self) -> Option<f64> {
        if self.reserved_marker.is_some() {
            return None;
        }
        let elapsed_s = self.elapsed_s(self.session_sample_count);
        if let Some(bands) = &mut self.bands {
            bands.reserve();
        }
        self.reserved_marker = Some(ReservedMarker {
            elapsed_s,
            sample_count: self.a.sample_count(),
            a: self.a.clone(),
            c: self.c.clone(),
            z: self.z.clone(),
            clipped: self.segment_clipped,
            following_a: SegmentLevels::default(),
            following_c: SegmentLevels::default(),
            following_z: SegmentLevels::default(),
            following_clipped: false,
        });
        Some(elapsed_s)
    }

    pub(crate) fn marker_at(
        &mut self,
        label: impl Into<String>,
        elapsed_s: f64,
        calibration_offset_db: f64,
    ) -> Result<(), ()> {
        let Some(marker) = self.reserved_marker.take() else {
            return Err(());
        };
        if elapsed_s.to_bits() != marker.elapsed_s.to_bits() {
            self.reserved_marker = Some(marker);
            return Err(());
        }
        let values = Self::values_from(&marker.a, &marker.c, &marker.z, calibration_offset_db);
        self.append_marker(
            MarkerKind::User,
            label,
            ClosedSegment {
                end_s: marker.elapsed_s,
                sample_count: marker.sample_count,
                clipped: marker.clipped,
                values,
                retroactive: true,
                calibration_offset_db,
            },
        );
        self.a = marker.following_a;
        self.c = marker.following_c;
        self.z = marker.following_z;
        self.segment_clipped = marker.following_clipped;
        Ok(())
    }

    pub(crate) fn cancel_marker_at(&mut self, elapsed_s: f64) -> Result<(), ()> {
        let Some(marker) = self.reserved_marker.as_ref() else {
            return Err(());
        };
        if elapsed_s.to_bits() != marker.elapsed_s.to_bits() {
            return Err(());
        }
        self.reserved_marker = None;
        if let Some(bands) = &mut self.bands {
            bands.cancel_reserved();
        }
        Ok(())
    }
}
