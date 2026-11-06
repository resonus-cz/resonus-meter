//! The log rows kept in memory for the Log view (ADR 0014).
//!
//! The full log lives in the journal. For plotting, memory holds the last
//! [`RECENT_S`] at full resolution and an overview of the whole session of
//! at most [`OVERVIEW_ROWS`] points, so a 24 h session costs a few megabytes.

use std::collections::VecDeque;

use spl_session::LogRecord;

/// Rows kept at full resolution: the longest finite Log view window.
pub(crate) const RECENT_S: f64 = 3_600.0;
/// Most points in the overview; a wide screen shows fewer pixels.
pub(crate) const OVERVIEW_ROWS: usize = 4_096;

#[derive(Debug, Clone, Default)]
pub(crate) struct PlotHistory {
    recent: VecDeque<LogRecord>,
    /// Whether rows have left `recent`, which then no longer covers the session.
    recent_trimmed: bool,
    /// Each point combines `group` consecutive rows.
    overview: Vec<LogRecord>,
    group: usize,
    /// The point being filled and the number of rows in it.
    open: Option<(LogRecord, usize)>,
    rows: usize,
}

impl PlotHistory {
    pub(crate) fn push(&mut self, row: LogRecord) {
        self.rows += 1;
        self.recent.push_back(row);
        while self
            .recent
            .front()
            .is_some_and(|first| first.t_s < row.t_s - RECENT_S)
        {
            self.recent.pop_front();
            self.recent_trimmed = true;
        }
        let group = self.group.max(1);
        let (point, count) = match self.open.take() {
            Some((point, count)) => (combine(point, row), count + 1),
            None => (row, 1),
        };
        if count < group {
            self.open = Some((point, count));
            return;
        }
        self.overview.push(point);
        if self.overview.len() >= OVERVIEW_ROWS {
            self.overview = self
                .overview
                .chunks(2)
                .map(|pair| pair.iter().copied().reduce(combine).unwrap_or(pair[0]))
                .collect();
            self.group = group * 2;
        }
    }

    /// Notes `marker_id` in the first row after `t_s` without one, as
    /// for a marker placed at an earlier reserved time.
    pub(crate) fn note_marker(&mut self, t_s: f64, marker_id: u64) {
        let unnoted = |row: &&mut LogRecord| row.t_s > t_s && row.marker_id.is_none();
        if let Some(row) = self.recent.iter_mut().find(unnoted) {
            row.marker_id = Some(marker_id);
        }
        let overview = self
            .overview
            .iter_mut()
            .chain(self.open.as_mut().map(|(point, _)| point));
        if let Some(point) = overview.into_iter().find(unnoted) {
            point.marker_id = Some(marker_id);
        }
    }

    /// Number of rows logged, including those no longer held.
    pub(crate) const fn rows(&self) -> usize {
        self.rows
    }

    /// Returns the rows for a view of the last `window_s` (all with `None`):
    /// full resolution when memory still holds them, otherwise the overview.
    /// The row before the window is included so the trace reaches its edge.
    pub(crate) fn window(&self, window_s: Option<f64>) -> Vec<LogRecord> {
        let Some(last) = self.recent.back() else {
            return Vec::new();
        };
        match window_s {
            Some(window_s) if window_s <= RECENT_S => {
                let first = self
                    .recent
                    .partition_point(|row| row.t_s < last.t_s - window_s)
                    .saturating_sub(1);
                self.recent.range(first..).copied().collect()
            }
            _ if !self.recent_trimmed => self.recent.iter().copied().collect(),
            _ => self
                .overview
                .iter()
                .copied()
                .chain(self.open.map(|(point, _)| point))
                .collect(),
        }
    }
}

/// One point for two consecutive rows: the later time and the highest
/// levels, so short loud events stay visible in the overview.
fn combine(earlier: LogRecord, later: LogRecord) -> LogRecord {
    LogRecord {
        t_s: later.t_s,
        utc: later.utc,
        marker_id: earlier.marker_id.or(later.marker_id),
        clipped: earlier.clipped || later.clipped,
        laf_db: earlier.laf_db.max(later.laf_db),
        lcf_db: earlier.lcf_db.max(later.lcf_db),
        lzf_db: earlier.lzf_db.max(later.lzf_db),
    }
}

#[cfg(test)]
mod tests {
    use super::{OVERVIEW_ROWS, PlotHistory, RECENT_S};
    use spl_session::LogRecord;
    use time::OffsetDateTime;

    fn row(t_s: f64, laf_db: f64) -> LogRecord {
        LogRecord {
            t_s,
            utc: OffsetDateTime::UNIX_EPOCH,
            marker_id: None,
            clipped: false,
            laf_db,
            lcf_db: laf_db,
            lzf_db: laf_db,
        }
    }

    #[test]
    fn a_short_session_is_shown_whole_at_full_resolution() {
        let mut history = PlotHistory::default();
        for second in 1..=100 {
            history.push(row(f64::from(second), 40.0));
        }

        assert_eq!(history.window(None).len(), 100);
        let last_minute = history.window(Some(60.0));
        assert_eq!(last_minute.len(), 62, "the window and the row before it");
        assert!((last_minute[0].t_s - 39.0).abs() < 1e-9);
    }

    #[test]
    fn a_long_session_keeps_the_last_hour_and_a_bounded_overview() {
        let mut history = PlotHistory::default();
        let rows = 3 * OVERVIEW_ROWS + 5;
        for index in 1..=rows {
            let t_s = f64::from(u32::try_from(index).expect("small"));
            // One loud row among quiet ones must survive the merging.
            history.push(row(t_s, if index == 1_000 { 90.0 } else { 40.0 }));
        }

        assert_eq!(history.rows(), rows);
        let overview = history.window(None);
        assert!(overview.len() <= OVERVIEW_ROWS, "{} points", overview.len());
        assert!(overview.len() > OVERVIEW_ROWS / 4);
        assert!(overview.iter().any(|point| point.laf_db > 89.0));
        assert!((overview.last().expect("points").t_s - t_s(rows)).abs() < 1e-9);
        let hour = history.window(Some(RECENT_S));
        assert_eq!(hour.len(), 3_601);
        assert!(history.recent.len() <= 3_601);
    }

    fn t_s(index: usize) -> f64 {
        f64::from(u32::try_from(index).expect("small"))
    }
}
