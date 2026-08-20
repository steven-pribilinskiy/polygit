//! Frame/input instrumentation for diagnosing UI sluggishness.
//!
//! The point is to SEPARATE the three things that all present as "the hover lags":
//!
//! 1. `build` — laying out widgets into ratatui's back buffer. Our code.
//! 2. `flush` — diffing against the front buffer and writing escape sequences to the tty, then
//!    waiting for the write to drain. The terminal emulator's speed, not ours.
//! 3. `backlog` — how many motion reports were superseded before we could draw them. A backlog
//!    means the terminal produces motion faster than the loop consumes it, so the highlight trails
//!    the cursor by `backlog * frame_time` no matter how fast a single frame is.
//!
//! `lag` is the user-visible symptom: wall time from reading a mouse-motion report to finishing the
//! frame that acts on it. A high `lag` with low `build`/`flush` and a high `backlog` means the loop
//! is structurally behind (one event per frame), not that any single step is slow.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How many samples each channel keeps. At ~60 fps this is roughly the last 4 seconds.
const WINDOW: usize = 256;

/// A rolling window of microsecond samples with order-statistic queries.
#[derive(Debug, Default, Clone)]
pub struct Channel {
    samples: VecDeque<f64>,
    /// Total observations ever recorded (the window only keeps the last `WINDOW`).
    pub count: u64,
    /// Largest sample ever seen, in microseconds — survives the window so a rare stall is not lost.
    pub peak: f64,
}

impl Channel {
    pub fn record(&mut self, value: Duration) {
        self.record_us(value.as_secs_f64() * 1e6);
    }

    pub fn record_us(&mut self, micros: f64) {
        if self.samples.len() == WINDOW {
            self.samples.pop_front();
        }
        self.samples.push_back(micros);
        self.count += 1;
        if micros > self.peak {
            self.peak = micros;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The `q`-quantile (0.0..=1.0) of the current window, by nearest-rank on a sorted copy.
    pub fn quantile(&self, q: f64) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let mut sorted: Vec<f64> = self.samples.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let rank = (q * (sorted.len() - 1) as f64).round() as usize;
        sorted[rank.min(sorted.len() - 1)]
    }

    pub fn p50(&self) -> f64 {
        self.quantile(0.50)
    }

    pub fn p95(&self) -> f64 {
        self.quantile(0.95)
    }

    pub fn p99(&self) -> f64 {
        self.quantile(0.99)
    }

    /// Largest sample still inside the window (distinct from `peak`, which never decays).
    pub fn window_max(&self) -> f64 {
        self.samples.iter().copied().fold(0.0_f64, f64::max)
    }
}

/// A counter whose per-second rate is derived from a moving wall-clock window.
#[derive(Debug, Clone, Default)]
pub struct Rate {
    events: VecDeque<Instant>,
    /// Every observation since start, not just those inside the window.
    pub total: u64,
    /// Highest per-second reading ever observed. The live window is always empty by the time the
    /// on-quit report prints — seconds have passed since the last event — so the report would
    /// otherwise always say zero, which reads as "no motion happened" rather than "not right now".
    pub peak_per_sec: usize,
}

impl Rate {
    pub fn tick(&mut self, now: Instant) {
        self.total += 1;
        self.events.push_back(now);
        self.trim(now);
        if self.events.len() > self.peak_per_sec {
            self.peak_per_sec = self.events.len();
        }
    }

    fn trim(&mut self, now: Instant) {
        while self.events.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(1)) {
            self.events.pop_front();
        }
    }

    /// Observations in the last second.
    pub fn per_sec(&mut self, now: Instant) -> usize {
        self.trim(now);
        self.events.len()
    }
}

/// Everything the event loop and the renderer report into. Lives on `AppState`.
///
/// Collection is gated on `enabled` so a release session pays nothing but a branch: the timers are
/// only read when the overlay (or the perf report) is on.
#[derive(Debug, Default, Clone)]
pub struct Perf {
    /// Whether to collect. Toggled by the `Ctrl+T` overlay / `--perf`.
    pub enabled: bool,
    /// Whether to draw the overlay. Collection can run without it (for the on-quit report).
    pub overlay: bool,

    /// Widget layout + paint into the back buffer (our code).
    pub build: Channel,
    /// The palette remap pass over every cell.
    pub palette: Channel,
    /// The hover-highlight pass.
    pub hover: Channel,
    /// Buffer diff + escape-sequence write + flush to the tty (the terminal's speed).
    pub flush: Channel,
    /// `build` + `flush`, i.e. the whole `terminal.draw` call.
    pub frame: Channel,
    /// Wall time for one full pass of the event loop, draw included.
    pub iter: Channel,
    /// Everything in the loop body that is NOT the draw (state upkeep, dwell tooltip, spawns).
    pub upkeep: Channel,
    /// Time spent blocked acquiring the `AppState` mutex in the loop.
    pub lock_wait: Channel,
    /// Handling one input event (dispatch, not the frame that follows).
    pub event: Channel,

    /// Motion report → end of the frame that acted on it. The user-visible hover lag.
    pub lag: Channel,
    /// Input events already queued when we polled. Non-zero means we are behind.
    pub backlog: Channel,
    /// Motion reports consumed but never drawn because a newer one superseded them.
    pub coalesced: Channel,

    /// Mouse-motion reports read per second.
    pub motion_rate: Rate,
    /// Frames drawn per second.
    pub frame_rate: Rate,
    /// Input events of any kind read per second.
    pub event_rate: Rate,

    /// Terminal round-trip: a Device Status Report sent, and the reply read back. Measured once at
    /// startup — it is the floor on how fast this terminal can possibly acknowledge anything.
    pub terminal_rtt: Option<Duration>,
    /// Cells the terminal is being asked to hold (width × height), for context on flush cost.
    pub cells: u32,
    /// The most recent frame's build time. The event loop subtracts it from the whole `draw` call
    /// to attribute the remainder to the flush, so it must be the CURRENT frame's value, not a
    /// window statistic.
    pub last_build: Duration,

    /// When the oldest not-yet-drawn motion report was read. Cleared by the frame that draws it.
    pending_motion: Option<Instant>,
    /// Motion reports dropped since the last drawn frame.
    pending_coalesced: u32,
}

impl Perf {
    /// Start collecting. Idempotent.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Toggle the on-screen overlay; turning it on also turns collection on, since an overlay over
    /// stale numbers is worse than none.
    pub fn toggle_overlay(&mut self) {
        self.overlay = !self.overlay;
        if self.overlay {
            self.enabled = true;
        }
    }

    /// Record that a mouse-motion report was read. The FIRST one after a frame anchors the lag
    /// measurement; later ones before that frame lands are counted as coalesced, because the
    /// highlight they asked for is never shown — it is superseded before it reaches the screen.
    pub fn motion_read(&mut self, at: Instant) {
        if !self.enabled {
            return;
        }
        self.motion_rate.tick(at);
        if self.pending_motion.is_none() {
            self.pending_motion = Some(at);
        } else {
            self.pending_coalesced += 1;
        }
    }

    /// Record an input event of any kind, with how many more were already queued behind it.
    pub fn event_read(&mut self, at: Instant, queued: usize) {
        if !self.enabled {
            return;
        }
        self.event_rate.tick(at);
        self.backlog.record_us(queued as f64);
    }

    /// Close out a frame: attributes the elapsed lag to the motion report that caused it.
    pub fn frame_done(&mut self, at: Instant) {
        if !self.enabled {
            return;
        }
        self.frame_rate.tick(at);
        if let Some(started) = self.pending_motion.take() {
            self.lag.record(at.saturating_duration_since(started));
            self.coalesced.record_us(f64::from(self.pending_coalesced));
            self.pending_coalesced = 0;
        }
    }

    /// The one-line verdict: which of the three suspects the numbers actually indict.
    ///
    /// Thresholds are deliberately coarse — this is a pointer to the right subsystem, not a score.
    /// 16 ms is one frame at 60 Hz; 100 ms is roughly where a pointer highlight stops feeling
    /// attached to the cursor.
    pub fn verdict(&self) -> &'static str {
        if self.lag.is_empty() {
            return "no motion sampled yet — move the mouse over the list";
        }
        let lag = self.lag.p95();
        if lag < 50_000.0 {
            return "hover is keeping up";
        }
        let backlog = self.backlog.p95();
        let build = self.build.p95();
        let flush = self.flush.p95();
        if backlog >= 2.0 && build + flush < 16_000.0 {
            "INPUT BACKLOG — the loop draws one frame per motion report; coalesce them"
        } else if flush > build * 2.0 && flush > 8_000.0 {
            "TERMINAL FLUSH — the emulator is the slow part, not the layout"
        } else if build > 8_000.0 {
            "FRAME BUILD — widget layout dominates; profile the render path"
        } else {
            "lag without an obvious single cause — check upkeep and lock_wait"
        }
    }

    /// A plain-text report, printed on quit when instrumentation ran.
    pub fn report(&self) -> String {
        let mut out = String::new();
        out.push_str("polygit perf report\n");
        out.push_str("===================\n\n");
        if let Some(rtt) = self.terminal_rtt {
            out.push_str(&format!(
                "terminal round-trip : {:.2} ms (DSR query → reply)\n",
                rtt.as_secs_f64() * 1e3
            ));
        }
        out.push_str(&format!("surface             : {} cells\n\n", self.cells));
        out.push_str(&format!(
            "{:<12} {:>9} {:>9} {:>9} {:>9} {:>9} {:>8}\n",
            "channel", "p50", "p95", "p99", "win-max", "peak", "n"
        ));
        let rows: [(&str, &Channel); 9] = [
            ("hover lag", &self.lag),
            ("frame", &self.frame),
            ("  build", &self.build),
            ("    palette", &self.palette),
            ("    hover", &self.hover),
            ("  flush", &self.flush),
            ("upkeep", &self.upkeep),
            ("lock wait", &self.lock_wait),
            ("event", &self.event),
        ];
        for (name, channel) in rows {
            if channel.is_empty() {
                continue;
            }
            out.push_str(&format!(
                "{:<12} {:>8.2}m {:>8.2}m {:>8.2}m {:>8.2}m {:>8.2}m {:>8}\n",
                name,
                channel.p50() / 1000.0,
                channel.p95() / 1000.0,
                channel.p99() / 1000.0,
                channel.window_max() / 1000.0,
                channel.peak / 1000.0,
                channel.count
            ));
        }
        out.push('\n');
        out.push_str(&format!(
            "superseded at poll  : p50 {:.1}  p95 {:.1}  max {:.0} motion reports dropped before \
             they could be drawn\n",
            self.backlog.p50(),
            self.backlog.p95(),
            self.backlog.peak
        ));
        // Only ever non-zero when coalescing is disabled: with it on, superseded reports are
        // dropped before they reach the frame accounting above.
        out.push_str(&format!(
            "undrawn per frame   : p50 {:.1}  p95 {:.1}  max {:.0} motion reports read but not \
             drawn\n",
            self.coalesced.p50(),
            self.coalesced.p95(),
            self.coalesced.peak
        ));
        out.push_str(&format!(
            "peak rates          : {} motion/s   {} event/s   {} frame/s\n",
            self.motion_rate.peak_per_sec,
            self.event_rate.peak_per_sec,
            self.frame_rate.peak_per_sec,
        ));
        out.push_str(&format!(
            "totals              : {} motion   {} events   {} frames\n",
            self.motion_rate.total, self.event_rate.total, self.frame_rate.total,
        ));
        out.push_str(&format!("\nverdict: {}\n", self.verdict()));
        out
    }
}

/// Measure how long this terminal takes to answer a Device Status Report — the floor on its
/// responsiveness, independent of anything polygit does.
///
/// Sends `CSI 6n` (report cursor position) and reads until the `R` terminator. Returns `None` if
/// the terminal never answers within `timeout`, which is itself a finding: a terminal that ignores
/// DSR is one whose latency cannot be separated from ours this way.
///
/// Must run while the terminal is in raw mode and BEFORE any other reader is consuming stdin,
/// otherwise the reply is stolen by the event reader.
pub fn probe_terminal_rtt(timeout: Duration) -> Option<Duration> {
    use std::io::{Read, Write};

    let mut stdout = std::io::stdout();
    let started = Instant::now();
    stdout.write_all(b"\x1b[6n").ok()?;
    stdout.flush().ok()?;

    let mut stdin = std::io::stdin();
    let mut buf = [0_u8; 1];
    let mut seen = Vec::new();
    while started.elapsed() < timeout {
        if !crossterm::event::poll(Duration::from_millis(10)).unwrap_or(false) {
            continue;
        }
        match stdin.read(&mut buf) {
            Ok(1) => {
                seen.push(buf[0]);
                if buf[0] == b'R' {
                    return Some(started.elapsed());
                }
            }
            Ok(_) => return None,
            Err(_) => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_track_the_window_not_all_history() {
        let mut channel = Channel::default();
        // A huge early sample must leave `peak` but fall out of the quantile window.
        channel.record_us(1_000_000.0);
        for _ in 0..WINDOW {
            channel.record_us(10.0);
        }
        assert_eq!(channel.p50(), 10.0);
        assert_eq!(channel.p99(), 10.0);
        assert_eq!(channel.peak, 1_000_000.0, "peak survives the window");
        assert_eq!(channel.count, WINDOW as u64 + 1);
        assert_eq!(channel.window_max(), 10.0);
    }

    #[test]
    fn empty_channel_answers_zero_rather_than_panicking() {
        let channel = Channel::default();
        assert!(channel.is_empty());
        assert_eq!(channel.p50(), 0.0);
        assert_eq!(channel.p95(), 0.0);
        assert_eq!(channel.window_max(), 0.0);
    }

    /// The lag measurement anchors on the FIRST motion report of a frame, and counts the rest as
    /// coalesced — those asked for a highlight that never reached the screen.
    #[test]
    fn lag_anchors_on_the_oldest_undrawn_motion() {
        let mut perf = Perf { enabled: true, ..Perf::default() };
        let start = Instant::now();
        perf.motion_read(start);
        perf.motion_read(start);
        perf.motion_read(start);
        perf.frame_done(start + Duration::from_millis(120));

        assert_eq!(perf.lag.count, 1, "one frame closes one lag sample");
        assert!(perf.lag.p50() >= 119_000.0, "lag measured from the oldest report");
        assert_eq!(perf.coalesced.p50(), 2.0, "the two superseded reports are counted");
        assert_eq!(perf.motion_rate.total, 3);
    }

    /// A frame with no motion behind it must not invent a lag sample.
    #[test]
    fn frame_without_motion_records_no_lag() {
        let mut perf = Perf { enabled: true, ..Perf::default() };
        perf.frame_done(Instant::now());
        assert!(perf.lag.is_empty());
        assert_eq!(perf.frame_rate.total, 1);
    }

    /// Collection is a no-op while disabled — the overlay must never show numbers from a period
    /// when it was off.
    #[test]
    fn disabled_perf_collects_nothing() {
        let mut perf = Perf::default();
        let now = Instant::now();
        perf.motion_read(now);
        perf.event_read(now, 12);
        perf.frame_done(now);
        assert!(perf.lag.is_empty());
        assert!(perf.backlog.is_empty());
        assert_eq!(perf.motion_rate.total, 0);
    }

    /// The verdict must name the backlog when frames are cheap but events are piling up — that is
    /// the one-frame-per-motion-report failure this module exists to surface.
    #[test]
    fn verdict_indicts_backlog_when_frames_are_cheap() {
        let mut perf = Perf { enabled: true, ..Perf::default() };
        for _ in 0..32 {
            perf.lag.record_us(300_000.0);
            perf.backlog.record_us(40.0);
            perf.build.record_us(1_500.0);
            perf.flush.record_us(2_000.0);
        }
        assert!(perf.verdict().starts_with("INPUT BACKLOG"), "got: {}", perf.verdict());
    }

    /// ...and must indict the terminal instead when the flush is what dominates.
    #[test]
    fn verdict_indicts_flush_when_the_terminal_is_slow() {
        let mut perf = Perf { enabled: true, ..Perf::default() };
        for _ in 0..32 {
            perf.lag.record_us(300_000.0);
            perf.backlog.record_us(0.0);
            perf.build.record_us(1_000.0);
            perf.flush.record_us(60_000.0);
        }
        assert!(perf.verdict().starts_with("TERMINAL FLUSH"), "got: {}", perf.verdict());
    }

    #[test]
    fn verdict_stays_quiet_when_hover_keeps_up() {
        let mut perf = Perf { enabled: true, ..Perf::default() };
        for _ in 0..32 {
            perf.lag.record_us(4_000.0);
            perf.backlog.record_us(0.0);
        }
        assert_eq!(perf.verdict(), "hover is keeping up");
    }

    #[test]
    fn rate_window_expires_old_observations_but_keeps_total_and_peak() {
        let mut rate = Rate::default();
        let base = Instant::now();
        rate.tick(base);
        rate.tick(base);
        assert_eq!(rate.per_sec(base), 2);
        // Two seconds later the old ticks have aged out, but total and peak are cumulative — the
        // on-quit report reads those, and a zeroed live window must not erase the measurement.
        assert_eq!(rate.per_sec(base + Duration::from_secs(2)), 0);
        assert_eq!(rate.total, 2);
        assert_eq!(rate.peak_per_sec, 2);
    }
}
