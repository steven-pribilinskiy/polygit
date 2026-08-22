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

/// How many rows a greedy word wrap of `text` to `width` produces. Mirrors the panel's own
/// wrapping so the reserved height and the drawn height agree — a mismatch clips the verdict.
fn wrapped_rows(text: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let mut rows = 0;
    let mut current = 0usize;
    for word in text.split_whitespace() {
        let len = word.chars().count();
        if current == 0 {
            current = len;
        } else if current + 1 + len <= width {
            current += 1 + len;
        } else {
            rows += 1;
            current = len;
        }
        while current > width {
            rows += 1;
            current -= width;
        }
    }
    if current > 0 {
        rows += 1;
    }
    rows.max(1)
}

/// Which series the history graph is plotting.
///
/// Each metric owns BOTH of its reductions, because they differ per metric and getting either
/// backwards is invisible in the output: a graph of frame time reduced by `Min` looks like a
/// perfectly healthy app, and a graph of FPS reduced by `Max` looks the same. They are not the
/// caller's choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Worst whole-frame time in the second. The default: it is recorded on every frame, so it is
    /// meaningful even when nothing is moving, and it is the thing that actually degrades.
    FrameTime,
    /// Worst motion-report-to-drawn-frame latency. Honestly blank while the mouse is still.
    HoverLag,
    /// Worst buffer-diff-and-write time — the emulator's contribution.
    Flush,
    /// Frames drawn in the second.
    Fps,
    /// Most motion reports superseded before they could be drawn.
    Backlog,
}

/// How a second's many observations collapse into that second's single value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reduce {
    /// The worst observation in the second.
    Max,
    /// How many observations there were (a rate).
    Count,
}

/// How several seconds collapse into one drawn column, once the window is longer than the graph is
/// wide. Always toward the WORSE value — a mean would erase the 400 ms stall the panel exists to
/// show, which is the only reason anyone opens it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aggregate {
    /// Worse is higher (durations, backlog).
    Max,
    /// Worse is lower (frame rate).
    Min,
}

impl Metric {
    /// Every metric, in the order the picker offers them.
    pub const ALL: [Metric; 5] =
        [Metric::FrameTime, Metric::HoverLag, Metric::Flush, Metric::Fps, Metric::Backlog];

    pub fn label(self) -> &'static str {
        match self {
            Metric::FrameTime => "frame",
            Metric::HoverLag => "hover lag",
            Metric::Flush => "flush",
            Metric::Fps => "fps",
            Metric::Backlog => "backlog",
        }
    }

    /// The unit its plotted values carry, for the caption.
    pub fn unit(self) -> &'static str {
        match self {
            Metric::Fps => "/s",
            Metric::Backlog => "",
            _ => "ms",
        }
    }

    /// Whether the plotted value is a duration in microseconds (so the caption divides by 1000).
    pub fn is_duration(self) -> bool {
        matches!(self, Metric::FrameTime | Metric::HoverLag | Metric::Flush)
    }

    fn reduce(self) -> Reduce {
        match self {
            Metric::Fps => Reduce::Count,
            _ => Reduce::Max,
        }
    }

    /// Which direction is worse — the one a column keeps when it has to drop seconds.
    pub fn aggregate(self) -> Aggregate {
        match self {
            Metric::Fps => Aggregate::Min,
            _ => Aggregate::Max,
        }
    }
}

/// One second's observations of every metric.
#[derive(Debug, Clone, Copy, Default)]
struct Bucket {
    /// Per-metric worst observation and observation count, indexed by `Metric::ALL` position.
    worst: [f64; Metric::ALL.len()],
    count: [u32; Metric::ALL.len()],
    /// Set when this second overlapped a panel drag or an open panel menu — the panel perturbs
    /// what it is measuring, and a spike caused by dragging the graph is not a finding.
    perturbed: bool,
}

/// Per-second history, for the graph.
///
/// The ring advances on WALL TIME, not on sample arrival. That distinction is the whole design: a
/// ring that only moves when a sample lands compresses idle time, so a minute spent suspended in
/// `claude` or `lazygit` (which leave the alternate screen entirely) would silently vanish from the
/// x-axis and the seconds either side of it would appear adjacent. Skipped seconds become explicit
/// `None`s, which the graph draws as gaps rather than as zeros — an idle app must not look like a
/// catastrophic stall.
#[derive(Debug, Clone)]
pub struct Series {
    epoch: Instant,
    /// Newest last. `None` is a second in which nothing was observed.
    buckets: VecDeque<Option<Bucket>>,
    /// Index (seconds since `epoch`) of the newest bucket.
    head: u64,
    cap: usize,
}

impl Default for Series {
    fn default() -> Self {
        Self::new(Instant::now(), Self::MAX_SECONDS)
    }
}

impl Series {
    /// The longest window offered (15 minutes), which sets the ring size.
    pub const MAX_SECONDS: usize = 900;

    pub fn new(epoch: Instant, cap: usize) -> Self {
        Self { epoch, buckets: VecDeque::new(), head: 0, cap: cap.max(1) }
    }

    fn index_of(&self, at: Instant) -> u64 {
        at.saturating_duration_since(self.epoch).as_secs()
    }

    /// Move the ring forward to `now`, filling every second in between with an explicit gap.
    pub fn advance_to(&mut self, now: Instant) {
        let target = self.index_of(now);
        if self.buckets.is_empty() {
            self.head = target;
            self.buckets.push_back(None);
        }
        while self.head < target {
            self.head += 1;
            self.buckets.push_back(None);
        }
        while self.buckets.len() > self.cap {
            self.buckets.pop_front();
        }
    }

    /// Fold one observation of `metric` into the second containing `now`.
    pub fn record(&mut self, now: Instant, metric: Metric, value: f64) {
        self.advance_to(now);
        let slot = metric as usize;
        let Some(bucket) = self.buckets.back_mut() else {
            return;
        };
        let bucket = bucket.get_or_insert_with(Bucket::default);
        bucket.count[slot] += 1;
        if value > bucket.worst[slot] {
            bucket.worst[slot] = value;
        }
    }

    /// One value per second for `metric`, oldest first, over the last `seconds`.
    fn seconds(&self, metric: Metric, seconds: usize) -> Vec<Option<f64>> {
        let slot = metric as usize;
        let reduce = metric.reduce();
        self.buckets
            .iter()
            .rev()
            .take(seconds)
            .map(|bucket| {
                bucket.and_then(|bucket| {
                    if bucket.count[slot] == 0 {
                        return None;
                    }
                    Some(match reduce {
                        Reduce::Max => bucket.worst[slot],
                        Reduce::Count => f64::from(bucket.count[slot]),
                    })
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// The plotted series: `seconds` of history downsampled into at most `columns` values, oldest
    /// first. A column with no observed second is `None` and draws as a gap.
    pub fn window(&self, metric: Metric, seconds: usize, columns: usize) -> Vec<Option<u64>> {
        let values = self.seconds(metric, seconds);
        if columns == 0 || values.is_empty() {
            return Vec::new();
        }
        if values.len() <= columns {
            return values.into_iter().map(|v| v.map(|v| v.round() as u64)).collect();
        }
        let per = values.len().div_ceil(columns);
        let aggregate = metric.aggregate();
        values
            .chunks(per)
            .map(|chunk| {
                chunk
                    .iter()
                    .filter_map(|value| *value)
                    .reduce(|a, b| match aggregate {
                        Aggregate::Max => a.max(b),
                        Aggregate::Min => a.min(b),
                    })
                    .map(|value| value.round() as u64)
            })
            .collect()
    }

    /// The `(low, high)` of the plotted window, for the caption. `None` when nothing was observed —
    /// a graph without its scale is unreadable, so the caller must be able to say so.
    pub fn range(&self, metric: Metric, seconds: usize) -> Option<(f64, f64)> {
        let mut low = f64::MAX;
        let mut high = f64::MIN;
        for value in self.seconds(metric, seconds).into_iter().flatten() {
            low = low.min(value);
            high = high.max(value);
        }
        (low <= high).then_some((low, high))
    }

    /// Whether any second in the window was perturbed by the panel's own use.
    pub fn window_perturbed(&self, seconds: usize) -> bool {
        self.buckets
            .iter()
            .rev()
            .take(seconds)
            .any(|bucket| bucket.is_some_and(|bucket| bucket.perturbed))
    }
}

/// What the history graph is showing. Persisted, so the panel comes back the way it was left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GraphPrefs {
    pub metric: Metric,
    /// How many seconds of history the graph covers.
    pub window_secs: u16,
    /// How many terminal rows the graph occupies. More rows is more vertical resolution — the
    /// sparkline packs 8 levels into each one.
    pub rows: u16,
}

impl Default for GraphPrefs {
    fn default() -> Self {
        // Frame time, not FPS: the loop draws unconditionally on a 50 ms poll, so an idle frame
        // rate is exactly 20 forever — a constant, not a signal — and it rises whenever the mouse
        // moves, which makes an FPS graph a graph of whether anyone is touching the mouse.
        Self { metric: Metric::FrameTime, window_secs: 60, rows: 5 }
    }
}

impl GraphPrefs {
    /// The windows the picker offers, and their labels.
    pub const WINDOWS: [(u16, &'static str); 4] =
        [(30, "30s"), (60, "1m"), (300, "5m"), (900, "15m")];

    /// The window clamped to what the ring can actually hold — a persisted value from a future
    /// build must not make the graph read past the end of its own history.
    pub fn seconds(self) -> usize {
        usize::from(self.window_secs).clamp(1, Series::MAX_SECONDS)
    }

    pub fn window_label(self) -> &'static str {
        Self::WINDOWS
            .iter()
            .find(|(secs, _)| *secs == self.window_secs)
            .map_or("custom", |(_, label)| *label)
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

    /// Widget layout + paint into the back buffer (our code). CONTAINS `palette` and `hover` —
    /// they are timed inside it, not beside it, so the three do not sum.
    pub build: Channel,
    /// The palette remap pass over every cell. Nested inside `build`.
    pub palette: Channel,
    /// The hover-highlight pass. Nested inside `build`.
    pub hover: Channel,
    /// Buffer diff + escape-sequence write + flush to the tty (the terminal's speed).
    pub flush: Channel,
    /// What the overlay itself costs to lay out. Subtracted from `flush`, because the overlay is
    /// drawn INSIDE `terminal.draw` — without this it is charged to the emulator, and the panel
    /// inflates the very channel `verdict` reads to blame the emulator.
    pub overlay_cost: Channel,
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
    /// The most recent frame's overlay-render time, for the same reason as `last_build`.
    pub last_overlay: Duration,

    /// Per-second history behind the graph.
    pub series: Series,
    /// What the graph is showing.
    pub graph: GraphPrefs,

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

    /// Record an input event of any kind — key, click, wheel, drag, resize, paste — with how many
    /// superseded motion reports the poll discarded ahead of it. Called for EVERY event, not only
    /// motion: gated behind the motion branch it counted the same stream as `motion_read`, so the
    /// panel showed one fact in two rows and `backlog` described only coalescing.
    pub fn event_read(&mut self, at: Instant, queued: usize) {
        if !self.enabled {
            return;
        }
        self.event_rate.tick(at);
        self.backlog.record_us(queued as f64);
        self.series.record(at, Metric::Backlog, queued as f64);
    }

    /// Fold one finished frame into the history. Called from the event loop, not from render, so
    /// the cadence is the loop's and the history survives the panel being closed.
    pub fn observe_frame(&mut self, at: Instant, frame: Duration, flush: Duration) {
        if !self.enabled {
            return;
        }
        self.series.record(at, Metric::FrameTime, frame.as_secs_f64() * 1e6);
        self.series.record(at, Metric::Flush, flush.as_secs_f64() * 1e6);
        self.series.record(at, Metric::Fps, 1.0);
    }

    /// Close out a frame: attributes the elapsed lag to the motion report that caused it.
    pub fn frame_done(&mut self, at: Instant) {
        if !self.enabled {
            return;
        }
        self.frame_rate.tick(at);
        if let Some(started) = self.pending_motion.take() {
            let lag = at.saturating_duration_since(started);
            self.series.record(at, Metric::HoverLag, lag.as_secs_f64() * 1e6);
            self.lag.record(lag);
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

    /// Every verdict this can return. The panel reserves height for the tallest so the box does
    /// not grow and shrink under the reader while the numbers move.
    pub const VERDICTS: [&'static str; 6] = [
        "no motion sampled yet — move the mouse over the list",
        "hover is keeping up",
        "INPUT BACKLOG — the loop draws one frame per motion report; coalesce them",
        "TERMINAL FLUSH — the emulator is the slow part, not the layout",
        "FRAME BUILD — widget layout dominates; profile the render path",
        "lag without an obvious single cause — check upkeep and lock_wait",
    ];

    /// How many wrapped rows the tallest verdict needs at `width`.
    pub fn max_verdict_rows(width: usize) -> usize {
        Self::VERDICTS.iter().map(|text| wrapped_rows(text, width)).max().unwrap_or(1).max(1)
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
        // Indentation is containment: `build` includes `palette` and `hover`, and `frame` includes
        // `build` and `flush`. `overlay` is the panel's own cost, already subtracted from `flush`.
        let rows: [(&str, &Channel); 10] = [
            ("hover lag", &self.lag),
            ("frame", &self.frame),
            ("  build", &self.build),
            ("    palette", &self.palette),
            ("    hover", &self.hover),
            ("  flush", &self.flush),
            ("overlay", &self.overlay_cost),
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

/// Split one `terminal.draw` call into what our layout cost and what the write cost.
///
/// `whole` is the entire call; `build` is the widget layout; `overlay` is the perf panel's own
/// render, which happens inside the same call and must not be billed to the terminal. Returns
/// `(frame, flush)` where `frame` is `whole` minus the overlay — the cost of drawing the app as it
/// would be with the panel closed — and `flush` is what is left after our layout.
///
/// The limit, stated because it cannot be measured away: the extra cells the panel puts into
/// ratatui's diff ARE real work for the emulator. This subtracts our layout cost, not the
/// emulator's cost of drawing us. A panel-open reading of `flush` is therefore still slightly
/// pessimistic, and the honest fix for a borderline verdict is to close the panel and use `--perf`.
///
/// Saturating throughout: the three clocks are read at different points, so a scheduler hiccup can
/// make the parts sum to more than the whole, and a negative duration would panic.
pub fn attribute_frame(whole: Duration, build: Duration, overlay: Duration) -> (Duration, Duration) {
    let frame = whole.saturating_sub(overlay);
    let flush = frame.saturating_sub(build);
    (frame, flush)
}

/// Which rows survive when the panel has fewer terminal rows than its full content wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelPlan {
    /// The history graph and its caption.
    pub graph: bool,
    /// Rows the graph occupies (0 when `graph` is false).
    pub graph_rows: u16,
    /// The per-second rate rows and the terminal round-trip.
    pub rates: bool,
    /// Upkeep, lock wait, overlay cost, dropped — useful, but not the headline.
    pub detail: bool,
    /// Total rows the panel will occupy, borders included.
    pub height: u16,
}

/// Core rows that are never dropped: the column header, hover lag, build, flush, backlog, and the
/// rule above the verdict.
const PANEL_CORE_ROWS: u16 = 6;

/// Decide what the panel can show in `available` terminal rows.
///
/// Drops in increasing order of value and NEVER the verdict, which is the one line the panel exists
/// to produce. The alternative — the all-or-nothing size check this replaced — meant that turning
/// the graph on made the panel vanish entirely for anyone whose terminal had been just big enough,
/// which is a worse answer than a smaller panel.
///
/// `None` when even the core does not fit; the caller then draws nothing rather than a box with the
/// verdict clipped off the bottom.
pub fn plan_panel(
    available: u16,
    verdict_rows: u16,
    graph_rows: u16,
    detail_rows: u16,
    rate_rows: u16,
) -> Option<PanelPlan> {
    // Two border rows, the core, and the reserved verdict.
    let floor = 2 + PANEL_CORE_ROWS + verdict_rows;
    if available < floor + 1 {
        return None;
    }
    let mut plan =
        PanelPlan { graph: false, graph_rows: 0, rates: false, detail: false, height: floor };
    let spare = |plan: &PanelPlan| available.saturating_sub(1).saturating_sub(plan.height);
    if detail_rows > 0 && spare(&plan) >= detail_rows {
        plan.detail = true;
        plan.height += detail_rows;
    }
    if rate_rows > 0 && spare(&plan) >= rate_rows {
        plan.rates = true;
        plan.height += rate_rows;
    }
    // The graph needs its rows plus a caption row, and is worth nothing without the caption: an
    // auto-normalised flat line at 20 fps looks identical to one at 120.
    let wanted = graph_rows.saturating_add(1);
    if graph_rows > 0 && spare(&plan) >= wanted {
        plan.graph = true;
        plan.graph_rows = graph_rows;
        plan.height += wanted;
    }
    Some(plan)
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

    /// The panel's usual tier sizes. The real caller counts its own rows instead — `term rtt` only
    /// exists when the terminal answered the probe, and planning for a row that is not there leaves
    /// a blank one — so these are the shape the tests exercise, not a production default.
    const PANEL_DETAIL_ROWS: u16 = 4;
    const PANEL_RATE_ROWS: u16 = 4;

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

    /// The overlay renders INSIDE `terminal.draw`, so without subtracting it the panel's own cost
    /// is billed to the terminal — inflating the one channel `verdict` uses to say the emulator is
    /// at fault. Assert the arithmetic, never a timing: a timing assertion here would be flaky and
    /// would prove nothing about the attribution.
    #[test]
    fn attribute_frame_bills_the_overlay_to_neither_build_nor_flush() {
        let whole = Duration::from_micros(1000);
        let build = Duration::from_micros(400);
        let overlay = Duration::from_micros(250);
        let (frame, flush) = attribute_frame(whole, build, overlay);
        assert_eq!(frame, Duration::from_micros(750), "frame excludes the overlay");
        assert_eq!(flush, Duration::from_micros(350), "flush is frame minus build");
        // The pre-fix behaviour charged the overlay to flush; that is what this must not do.
        assert_ne!(flush, whole.saturating_sub(build));
    }

    /// The three clocks are read at different points, so a scheduler hiccup can make the parts sum
    /// to more than the whole. Duration subtraction panics on underflow — saturate instead.
    #[test]
    fn attribute_frame_saturates_when_the_parts_exceed_the_whole() {
        let (frame, flush) = attribute_frame(
            Duration::from_micros(100),
            Duration::from_micros(900),
            Duration::from_micros(900),
        );
        assert_eq!(frame, Duration::ZERO);
        assert_eq!(flush, Duration::ZERO);
    }

    /// The ring advances on WALL TIME. A ring that only moves when a sample lands compresses idle
    /// time — a minute suspended in `claude`/`lazygit` would vanish and the seconds either side
    /// would look adjacent. The discrimination is the LENGTH: a compacting implementation returns
    /// two values here, not five.
    #[test]
    fn series_leaves_a_gap_for_seconds_in_which_nothing_ran() {
        let epoch = Instant::now();
        let mut series = Series::new(epoch, 64);
        series.record(epoch, Metric::FrameTime, 500.0);
        series.record(epoch + Duration::from_secs(4), Metric::FrameTime, 700.0);

        let window = series.window(Metric::FrameTime, 8, 8);
        assert_eq!(window, vec![Some(500), None, None, None, Some(700)]);
    }

    /// An idle second must be a gap, never a zero — a zero renders as a floor-height bar and makes
    /// an idle app look like a catastrophic stall.
    #[test]
    fn an_unobserved_second_is_none_not_zero() {
        let epoch = Instant::now();
        let mut series = Series::new(epoch, 64);
        series.advance_to(epoch + Duration::from_secs(3));
        assert!(series.window(Metric::FrameTime, 8, 8).iter().all(Option::is_none));
        assert_eq!(series.range(Metric::FrameTime, 8), None, "no scale without observations");
    }

    /// Downsampling keeps the WORST second in each column. A mean would erase exactly the spike the
    /// panel exists to show, so this plants one spike in 900 seconds and requires it to survive.
    #[test]
    fn series_downsampling_keeps_the_spike_a_mean_would_erase() {
        let epoch = Instant::now();
        let mut series = Series::new(epoch, Series::MAX_SECONDS);
        for second in 0..900_u64 {
            let value = if second == 700 { 500_000.0 } else { 400.0 };
            series.record(epoch + Duration::from_secs(second), Metric::FrameTime, value);
        }
        let window = series.window(Metric::FrameTime, 900, 20);
        assert!(window.len() <= 20, "downsampled to the column budget");
        assert_eq!(
            window.iter().filter(|v| **v == Some(500_000)).count(),
            1,
            "the spike survives into exactly one column: {window:?}"
        );
        // A mean over a 45-second chunk would land near 11_500 — nowhere near the real value.
        assert!(window.contains(&Some(500_000)));
    }

    /// FPS is worse when LOWER, so its columns keep the minimum. Reduced the same way as a duration
    /// it would report the best second in each chunk and a stuttering app would look smooth — and
    /// a same-direction test cannot see the difference.
    #[test]
    fn fps_columns_keep_the_worst_second_which_is_the_lowest() {
        let epoch = Instant::now();
        let mut series = Series::new(epoch, 64);
        // Second 0: 30 frames. Second 1: 3 frames (a stall). Second 2: 30 frames.
        for (second, frames) in [(0_u64, 30), (1, 3), (2, 30)] {
            for _ in 0..frames {
                series.record(epoch + Duration::from_secs(second), Metric::Fps, 1.0);
            }
        }
        assert_eq!(Metric::Fps.aggregate(), Aggregate::Min);
        let window = series.window(Metric::Fps, 3, 1);
        assert_eq!(window, vec![Some(3)], "the stall is what one column must show");
        // The frame-time metric goes the other way, and would have kept 30 here.
        assert_eq!(Metric::FrameTime.aggregate(), Aggregate::Max);
    }

    /// The ring is bounded: a long session must not grow without limit.
    #[test]
    fn series_drops_the_oldest_seconds_past_its_cap() {
        let epoch = Instant::now();
        let mut series = Series::new(epoch, 10);
        for second in 0..100_u64 {
            series.record(epoch + Duration::from_secs(second), Metric::FrameTime, second as f64);
        }
        let window = series.window(Metric::FrameTime, 100, 100);
        assert_eq!(window.len(), 10, "only the cap is retained");
        assert_eq!(window.last(), Some(&Some(99)), "and it is the newest that survives");
    }

    /// Nothing perturbs the history yet — the marker arrives with the drag that needs it. Until
    /// then the graph must not claim a disturbance it cannot have observed.
    #[test]
    fn history_is_unperturbed_until_something_marks_it() {
        let epoch = Instant::now();
        let mut series = Series::new(epoch, 64);
        series.record(epoch, Metric::FrameTime, 400.0);
        assert!(!series.window_perturbed(8));
    }

    /// The caption's scale comes from the observed values only — a gap must not drag the low to 0.
    #[test]
    fn the_scale_ignores_gaps() {
        let epoch = Instant::now();
        let mut series = Series::new(epoch, 64);
        series.record(epoch, Metric::FrameTime, 400.0);
        series.record(epoch + Duration::from_secs(5), Metric::FrameTime, 900.0);
        assert_eq!(series.range(Metric::FrameTime, 10), Some((400.0, 900.0)));
    }

    /// The verdict is the deliverable, so it is the last thing standing. At every height from the
    /// floor upward the plan must keep it — and must give up the graph before the numbers.
    #[test]
    fn panel_plan_drops_the_graph_before_the_numbers_and_never_the_verdict() {
        let verdict = 4;
        let floor = 2 + PANEL_CORE_ROWS + verdict;
        assert_eq!(plan_panel(floor, verdict, 5, PANEL_DETAIL_ROWS, PANEL_RATE_ROWS), None, "no room for the core: draw nothing");

        let tight = plan_panel(floor + 1, verdict, 5, PANEL_DETAIL_ROWS, PANEL_RATE_ROWS).expect("the core fits");
        assert!(!tight.graph && !tight.rates && !tight.detail, "everything optional is dropped");
        assert_eq!(tight.height, floor, "and the height is exactly the core");

        // Growing the terminal restores detail, then rates, then the graph — in that order.
        let mut seen = Vec::new();
        for available in (floor + 1)..(floor + 24) {
            let plan = plan_panel(available, verdict, 5, PANEL_DETAIL_ROWS, PANEL_RATE_ROWS).expect("still fits");
            seen.push((plan.detail, plan.rates, plan.graph));
            assert!(plan.height < available, "the plan never exceeds the space");
            if plan.graph {
                assert!(plan.rates && plan.detail, "the graph is the last thing added back");
            }
        }
        assert!(seen.contains(&(true, false, false)), "detail comes back first");
        assert!(seen.contains(&(true, true, false)), "then the rates");
        assert!(seen.contains(&(true, true, true)), "then the graph");
    }

    /// A graph with no caption cannot be read, so it is all-or-nothing with its caption row.
    #[test]
    fn the_graph_takes_its_caption_row_with_it() {
        let verdict = 1;
        let floor = 2 + PANEL_CORE_ROWS + verdict + PANEL_DETAIL_ROWS + PANEL_RATE_ROWS;
        // Exactly enough for five graph rows but not the caption: the graph is refused.
        let plan = plan_panel(floor + 5 + 1, verdict, 5, PANEL_DETAIL_ROWS, PANEL_RATE_ROWS).expect("fits");
        assert!(!plan.graph, "five rows without a sixth for the caption is not a graph");
        let plan = plan_panel(floor + 6 + 1, verdict, 5, PANEL_DETAIL_ROWS, PANEL_RATE_ROWS).expect("fits");
        assert!(plan.graph && plan.graph_rows == 5);
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
