//! Noticing when a window recording stops getting video.
//!
//! Apps stop drawing windows nobody can see. Every app stops for a
//! minimized window. Web-based ones (browsers, and apps built on WebKit or
//! Chromium) also stop, or slow to about one update a second, for a window
//! that is covered or on another desktop. Capture then gets few or no new
//! frames, and the recording holds the last one. Video counts as paused
//! when the window is out of view and fewer than [`MIN_FRAMES`] frames
//! arrived in the last [`STALE`], and stays paused until [`LIVE_FRAMES`]
//! arrive in one [`STALE`] or the window is back in view. A live call sends
//! far more; a window in view whose picture just isn't changing (a still
//! slide) is never paused.

use std::collections::VecDeque;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The span over which new frames are counted.
pub const STALE: Duration = Duration::from_secs(2);

/// Fewer frames than this in [`STALE`], out of view, is paused video.
pub const MIN_FRAMES: u64 = 3;

/// A paused window needs this many frames in [`STALE`] to count as live
/// again, so a page drawing about once a second doesn't flicker between
/// paused and live.
pub const LIVE_FRAMES: u64 = 8;

const STALE_MS: u64 = STALE.as_millis() as u64;

/// How much of a window can be seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowView {
    /// At least part of it is on screen.
    Visible,
    /// On screen, but entirely behind other windows.
    Covered,
    /// Minimized, or on a desktop that isn't showing.
    Hidden,
}

/// A stretch of a recording in which the window was out of view and sent
/// little or no new video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoPause {
    /// Milliseconds from the start of the recording.
    pub from_ms: u64,
    pub to_ms: u64,
    /// Where the window was when the pause ended.
    pub window: WindowView,
}

/// Turns periodic looks at a window into [`VideoPause`]s.
#[derive(Debug, Default)]
pub struct PauseTracker {
    /// (time, frames received so far), reaching back at least [`STALE`].
    counts: VecDeque<(u64, u64)>,
    /// Since when the window has been out of view, if it is.
    out_of_view_since: Option<u64>,
    /// The pause in progress: its start, and where the window is now.
    open: Option<(u64, WindowView)>,
    done: Vec<VideoPause>,
}

impl PauseTracker {
    /// Records one look at the window, `now_ms` into the recording, when
    /// `received` frames have arrived since the start. Returns where the
    /// window is while video is paused, or `None` while it is live.
    pub fn observe(&mut self, now_ms: u64, received: u64, view: WindowView) -> Option<WindowView> {
        let recent = self.recent(now_ms, received);
        if view == WindowView::Visible {
            self.out_of_view_since = None;
        } else if self.out_of_view_since.is_none() {
            self.out_of_view_since = Some(now_ms);
        }
        let out_of_view = self.out_of_view_since.is_some();

        match self.open {
            Some((from, _)) if out_of_view && recent.is_some_and(|n| n < LIVE_FRAMES) => {
                self.open = Some((from, view));
                Some(view)
            }
            // Video is back, or the window is in view again.
            Some((from, _)) => {
                self.close(from, now_ms);
                None
            }
            // It began when the frames dried up, or when the window went
            // out of view if that was later.
            None => match (recent, self.out_of_view_since) {
                (Some(n), Some(since)) if n < MIN_FRAMES => {
                    let from = since.max(now_ms.saturating_sub(STALE_MS));
                    self.open = Some((from, view));
                    Some(view)
                }
                _ => None,
            },
        }
    }

    /// Frames received in the last [`STALE`]; `None` until [`STALE`] has
    /// been watched.
    fn recent(&mut self, now_ms: u64, received: u64) -> Option<u64> {
        self.counts.push_back((now_ms, received));
        while self
            .counts
            .get(1)
            .is_some_and(|&(t, _)| now_ms.saturating_sub(t) >= STALE_MS)
        {
            self.counts.pop_front();
        }
        let (then, before) = self.counts[0];
        (now_ms.saturating_sub(then) >= STALE_MS).then(|| received.saturating_sub(before))
    }

    fn close(&mut self, from_ms: u64, to_ms: u64) {
        let window = self.open.take().map_or(WindowView::Hidden, |(_, w)| w);
        if to_ms > from_ms {
            self.done.push(VideoPause {
                from_ms,
                to_ms,
                window,
            });
        }
    }

    /// Ends the recording at `end_ms`, closing a pause still in progress.
    pub fn finish(mut self, end_ms: u64) -> Vec<VideoPause> {
        if let Some((from, _)) = self.open {
            self.close(from, end_ms);
        }
        self.done
    }
}

/// A rectangle in screen points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.x + self.width
    }

    fn bottom(&self) -> f64 {
        self.y + self.height
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

/// Whether `windows` together hide every point of `target`. The edges of
/// all the rectangles cut `target` into cells that are each either wholly
/// covered by some window or not at all, so testing one point per cell is
/// exact.
pub fn fully_covered(target: Rect, windows: &[Rect]) -> bool {
    if target.width <= 0.0 || target.height <= 0.0 {
        return false;
    }
    let cuts = |start: f64, end: f64, edges: &mut dyn Iterator<Item = f64>| {
        let mut v: Vec<f64> = [start, end]
            .into_iter()
            .chain(edges.filter(|&e| e > start && e < end))
            .collect();
        v.sort_by(f64::total_cmp);
        v.dedup();
        v
    };
    let xs = cuts(
        target.x,
        target.right(),
        &mut windows.iter().flat_map(|w| [w.x, w.right()]),
    );
    let ys = cuts(
        target.y,
        target.bottom(),
        &mut windows.iter().flat_map(|w| [w.y, w.bottom()]),
    );
    xs.windows(2).all(|x| {
        ys.windows(2).all(|y| {
            let (cx, cy) = ((x[0] + x[1]) / 2.0, (y[0] + y[1]) / 2.0);
            windows.iter().any(|w| w.contains(cx, cy))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use WindowView::{Covered, Hidden, Visible};

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Looks every 500 ms from `from` to `to` (exclusive), with `start`
    /// frames received at `from` and more arriving at `fps`; returns the
    /// last answer and the frame count.
    fn run(
        t: &mut PauseTracker,
        from: u64,
        to: u64,
        start: u64,
        fps: f64,
        view: WindowView,
    ) -> (Option<WindowView>, u64) {
        let (mut last, mut received) = (None, start);
        for now in (from..to).step_by(500) {
            received = start + ((now - from) as f64 * fps / 1000.0) as u64;
            last = t.observe(now, received, view);
        }
        (last, received)
    }

    #[test]
    fn coverage_needs_every_point_hidden() {
        let call = rect(100.0, 100.0, 800.0, 600.0);
        assert!(!fully_covered(call, &[]));
        assert!(fully_covered(call, &[rect(0.0, 0.0, 1920.0, 1080.0)]));
        // A strip 24 points wide is still showing.
        assert!(!fully_covered(call, &[rect(0.0, 0.0, 876.0, 1080.0)]));
        // Two windows that meet edge to edge cover it together.
        assert!(fully_covered(
            call,
            &[
                rect(0.0, 0.0, 500.0, 1080.0),
                rect(500.0, 0.0, 1420.0, 1080.0)
            ]
        ));
        // Four windows around a hole in the middle don't.
        assert!(!fully_covered(
            call,
            &[
                rect(0.0, 0.0, 1920.0, 400.0),
                rect(0.0, 500.0, 1920.0, 580.0),
                rect(0.0, 0.0, 450.0, 1080.0),
                rect(550.0, 0.0, 1370.0, 1080.0),
            ]
        ));
        // Windows on another display don't count.
        assert!(!fully_covered(call, &[rect(1920.0, 0.0, 2560.0, 1440.0)]));
    }

    #[test]
    fn live_video_out_of_view_is_not_paused() {
        let mut t = PauseTracker::default();
        assert_eq!(run(&mut t, 0, 10_000, 0, 15.0, Hidden).0, None);
        assert!(t.finish(10_000).is_empty());
    }

    #[test]
    fn a_still_picture_in_view_is_not_paused() {
        let mut t = PauseTracker::default();
        assert_eq!(run(&mut t, 0, 10_000, 0, 0.0, Visible).0, None);
        assert!(t.finish(10_000).is_empty());
    }

    #[test]
    fn covering_a_web_page_pauses_until_it_is_shown_again() {
        let mut t = PauseTracker::default();
        let (state, n) = run(&mut t, 0, 4_000, 0, 30.0, Visible);
        assert_eq!(state, None);
        // Covered at 4 s; the page drops to one update a second or so,
        // sometimes three in two seconds, which must not end the pause.
        assert_eq!(t.observe(4_000, n + 1, Covered), None, "too soon to tell");
        let mut seen = Vec::new();
        let mut n = n + 1;
        for now in (4_500..12_000).step_by(500) {
            n += u64::from(now % 1_000 == 0) + u64::from(now % 3_000 == 0);
            seen.push(t.observe(now, n, Covered));
        }
        let first = seen.iter().position(Option::is_some).expect("paused");
        assert!(
            seen[first..].iter().all(|s| *s == Some(Covered)),
            "{seen:?}"
        );
        // Then moved to another desktop, where it stops altogether.
        let (state, n) = run(&mut t, 12_000, 14_000, n, 0.0, Hidden);
        assert_eq!(state, Some(Hidden));
        // Brought back: it draws again.
        assert_eq!(t.observe(14_000, n + 10, Visible), None);
        assert_eq!(
            t.finish(16_000),
            [VideoPause {
                from_ms: 4_000,
                to_ms: 14_000,
                window: Hidden
            }]
        );
    }

    #[test]
    fn video_coming_back_out_of_view_ends_the_pause() {
        let mut t = PauseTracker::default();
        let (state, n) = run(&mut t, 0, 5_000, 0, 0.0, Hidden);
        assert_eq!(state, Some(Hidden));
        // A native app keeps drawing on another desktop once there is
        // something new to show.
        let (state, _) = run(&mut t, 5_000, 8_000, n, 15.0, Hidden);
        assert_eq!(state, None);
        let pauses = t.finish(8_000);
        assert_eq!(pauses.len(), 1);
        assert_eq!(pauses[0].from_ms, 0);
    }

    #[test]
    fn a_still_picture_then_hidden_pauses_from_when_it_was_hidden() {
        let mut t = PauseTracker::default();
        run(&mut t, 0, 30_000, 0, 0.0, Visible);
        assert_eq!(t.observe(30_000, 0, Hidden), Some(Hidden));
        // Back in view with the picture unchanged.
        assert_eq!(t.observe(40_000, 0, Visible), None);
        assert_eq!(
            t.finish(50_000),
            [VideoPause {
                from_ms: 30_000,
                to_ms: 40_000,
                window: Hidden
            }]
        );
    }

    #[test]
    fn a_pause_at_the_end_is_closed_by_finish() {
        let mut t = PauseTracker::default();
        let (state, _) = run(&mut t, 0, 3_000, 0, 0.0, Hidden);
        assert_eq!(state, Some(Hidden));
        assert_eq!(
            t.finish(12_000),
            [VideoPause {
                from_ms: 0,
                to_ms: 12_000,
                window: Hidden
            }]
        );
    }

    #[test]
    fn pauses_serialize_in_snake_case() {
        let json = serde_json::to_string(&VideoPause {
            from_ms: 1,
            to_ms: 2,
            window: Covered,
        })
        .unwrap();
        assert_eq!(json, r#"{"from_ms":1,"to_ms":2,"window":"covered"}"#);
    }
}
