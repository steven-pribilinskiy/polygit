//! The TUI's one reader of the terminal, and the light/dark follower fed from it.
//!
//! Input comes through `tuilith::input::EventStream`, not crossterm's reader. crossterm holds every
//! key typed after a mode-2031 colour-scheme report and types an OSC 11 reply in as keys, so with it
//! the running app could never ask the terminal what its background is — `Theme::Auto` used to
//! follow the Windows/macOS setting instead, which is a proxy for the terminal and a wrong one for a
//! dark terminal on a light desktop, and with `COLORFGBG` set it answered every poll with the same
//! fixed value. Everything a person does still arrives as crossterm's own `Event`, so the key and
//! mouse handling in `main.rs` is unchanged.
//!
//! The stream reads on a thread of its own from the moment it exists, which is the reason for
//! [`TerminalInput::suspend`]: a child that takes the terminal (`launch_claude`, `launch_lazygit`)
//! would otherwise have its keystrokes stolen by that thread.
//!
//! Every terminal query the TUI makes goes through here for the same reason — two readers race for
//! every byte. So the kitty keyboard probe is done here rather than with crossterm's
//! `supports_keyboard_enhancement`, and the perf panel's DSR round-trip likewise.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use tuilith::background::Reading;
use tuilith::follow::Follower;
use tuilith::input::{self, EventStream};
use tuilith::theme::Mode;

/// How long the kitty keyboard probe waits. Every terminal answers the DA1 sent behind the query, so
/// this is reached only by one that answers nothing at all.
const KEYBOARD_PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// The reader, and the follower once the TUI's screen is up.
struct Live {
    events: EventStream,
    /// `None` while the startup probes run: the follower's own queries would be answered into them.
    follower: Option<Follower>,
}

pub struct TerminalInput {
    /// `None` while suspended for a child process.
    live: Option<Live>,
    /// The terminal's light/dark and which signal said so. Survives a suspend, so the follower picks
    /// up where it left off.
    reading: Reading,
    /// What a person did while a probe was waiting for its reply, delivered before anything newer.
    held: VecDeque<Event>,
    /// Whether the kitty keyboard flags were pushed, so leaving pops exactly what entering pushed.
    keyboard_pushed: bool,
}

impl TerminalInput {
    /// Start reading the terminal. Call it once raw mode is on, and after
    /// `tuilith::background::read` — that probe reads its own reply from the tty.
    pub fn open(reading: Reading) -> io::Result<Self> {
        Ok(Self {
            live: Some(Live { events: EventStream::new()?, follower: None }),
            reading,
            held: VecDeque::new(),
            keyboard_pushed: false,
        })
    }

    /// The terminal's current light/dark, and which signal decided it.
    pub fn reading(&self) -> Reading {
        self.reading
    }

    /// Whether `Theme::Auto` should draw dark.
    pub fn dark(&self) -> bool {
        self.reading.mode == Mode::Dark
    }

    /// The TUI's screen is up: push the kitty keyboard flags where the terminal supports them (so
    /// Shift+Enter keeps its modifier and bare modifier presses arrive for the keyboard viewer), then
    /// start following the terminal's light/dark.
    pub fn enter(&mut self) -> io::Result<()> {
        if self.supports_keyboard_enhancement() {
            let _ = execute!(
                io::stdout(),
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                )
            );
            self.keyboard_pushed = true;
        }
        if let Some(live) = self.live.as_mut() {
            live.follower = Some(Follower::start(self.reading)?);
        }
        Ok(())
    }

    /// Hand the terminal over: pop the keyboard flags, stop the colour-scheme reports (the follower
    /// writes `follow::DISABLE` when dropped) and stop reading. Call it while still on the alternate
    /// screen — the kitty flag stack is per screen.
    pub fn suspend(&mut self) {
        if self.keyboard_pushed {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
            self.keyboard_pushed = false;
        }
        if let Some(follower) = self.live.take().and_then(|live| live.follower) {
            self.reading = follower.reading();
        }
    }

    /// Take the terminal back after [`suspend`](Self::suspend): raw mode and the alternate screen
    /// first, then this.
    pub fn resume(&mut self) -> io::Result<()> {
        if self.live.is_none() {
            self.live = Some(Live { events: EventStream::new()?, follower: None });
        }
        self.enter()
    }

    /// The next thing a person did, waiting at most `timeout`. Every terminal reply on the way is fed
    /// to the follower and never surfaces as input.
    pub fn next(&mut self, timeout: Duration) -> io::Result<Option<Event>> {
        if let Some(event) = self.held.pop_front() {
            return Ok(Some(event));
        }
        let Some(live) = self.live.as_mut() else {
            return Ok(None);
        };
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(event) = live.events.next_timeout(left)? else {
                return Ok(None);
            };
            if let Some(reading) =
                live.follower.as_mut().and_then(|follower| follower.observe(&event))
            {
                self.reading = reading;
            }
            if let input::Event::Terminal(event) = event {
                return Ok(Some(event));
            }
        }
    }

    /// Call once per loop iteration: asks the terminal (or the desktop) when that is due.
    pub fn tick(&mut self) {
        if let Some(reading) =
            self.live.as_mut().and_then(|live| live.follower.as_mut()).and_then(Follower::tick)
        {
            self.reading = reading;
        }
    }

    /// Measure how long this terminal takes to answer a Device Status Report — the floor on its
    /// responsiveness, independent of anything polygit does. `None` when it never answers within
    /// `timeout`, which is itself a finding: a terminal that ignores DSR is one whose latency cannot
    /// be separated from ours this way. Run it before [`enter`](Self::enter), so no follower query
    /// is in flight.
    pub fn probe_rtt(&mut self, timeout: Duration) -> Option<Duration> {
        let started = Instant::now();
        let mut stdout = io::stdout();
        stdout.write_all(b"\x1b[6n").ok()?;
        stdout.flush().ok()?;
        self.await_reply(started + timeout, |event| {
            matches!(event, input::Event::CursorPosition { .. }).then_some(())
        })?;
        Some(started.elapsed())
    }

    /// Ask for the kitty keyboard flags with DA1 behind them, as crossterm's own probe does: flags
    /// arriving before DA1 means supported. DA1 is waited for either way, so a late one cannot land
    /// after the follower has started and read as "this terminal does not answer OSC 11".
    fn supports_keyboard_enhancement(&mut self) -> bool {
        // The Windows console delivers input as records; no reply ever arrives through it.
        if cfg!(windows) {
            return false;
        }
        let mut stdout = io::stdout();
        if stdout.write_all(b"\x1b[?u\x1b[c").and_then(|()| stdout.flush()).is_err() {
            return false;
        }
        let mut flags = false;
        let deadline = Instant::now() + KEYBOARD_PROBE_TIMEOUT;
        self.await_reply(deadline, |event| match event {
            input::Event::KeyboardEnhancementFlags(_) => {
                flags = true;
                None
            }
            input::Event::DeviceAttributes => Some(()),
            _ => None,
        });
        flags
    }

    /// Read until `reply` accepts an event or `deadline` passes. What a person does meanwhile is held
    /// for [`next`](Self::next) rather than dropped; other terminal replies are discarded, since
    /// nothing is listening for them yet.
    fn await_reply<T>(
        &mut self,
        deadline: Instant,
        mut reply: impl FnMut(&input::Event) -> Option<T>,
    ) -> Option<T> {
        let live = self.live.as_ref()?;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = live.events.next_timeout(left).ok()??;
            if let Some(answer) = reply(&event) {
                return Some(answer);
            }
            if let input::Event::Terminal(event) = event {
                self.held.push_back(event);
            }
        }
    }
}
