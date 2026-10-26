use std::time::Instant;

use super::CatalogStamp;
use crate::{WATCH_DEBOUNCE, WATCH_HEARTBEAT, WATCH_MAX_DRAIN};

#[derive(Clone, Copy)]
pub(super) enum WakeMode {
    Events,
    Poll,
}

pub(super) struct ReloadWatch {
    pub wake: WakeMode,
    first_pending: Option<Instant>,
    last_event: Option<Instant>,
    last_stamp: CatalogStamp,
    last_heartbeat: Instant,
}

impl ReloadWatch {
    pub fn new(wake: WakeMode, last_stamp: CatalogStamp, now: Instant) -> Self {
        Self {
            wake,
            first_pending: None,
            last_event: None,
            last_stamp,
            last_heartbeat: now,
        }
    }

    pub fn on_event(&mut self, now: Instant) {
        self.first_pending.get_or_insert(now);
        self.last_event = Some(now);
    }

    pub fn should_reload(&mut self, now: Instant, stamp: impl FnOnce() -> CatalogStamp) -> bool {
        if !self.is_due(now) {
            return false;
        }
        self.skip(now);
        let current = stamp();
        if current == self.last_stamp {
            return false;
        }
        self.last_stamp = current;
        true
    }

    pub fn is_due(&self, now: Instant) -> bool {
        match self.wake {
            WakeMode::Events => {
                self.last_event
                    .is_some_and(|last| now.duration_since(last) >= WATCH_DEBOUNCE)
                    || self
                        .first_pending
                        .is_some_and(|first| now.duration_since(first) >= WATCH_MAX_DRAIN)
            }
            WakeMode::Poll => now.duration_since(self.last_heartbeat) >= WATCH_HEARTBEAT,
        }
    }

    pub fn record_reload(&mut self, now: Instant, stamp: CatalogStamp) {
        self.skip(now);
        self.last_stamp = stamp;
    }

    pub fn skip(&mut self, now: Instant) {
        self.first_pending = None;
        self.last_event = None;
        self.last_heartbeat = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn stamp(length: u64) -> CatalogStamp {
        CatalogStamp(vec![super::super::StampEntry {
            name: "flow.md".into(),
            modified: None,
            length,
        }])
    }

    #[test]
    fn stamp_error_waits_for_the_next_wake() {
        let now = Instant::now();
        let mut watch = ReloadWatch::new(WakeMode::Events, stamp(1), now);
        watch.on_event(now);
        let skipped_at = now + WATCH_DEBOUNCE;
        assert!(watch.is_due(skipped_at));
        watch.skip(skipped_at);
        assert!(!watch.is_due(skipped_at));
        assert!(!watch.is_due(skipped_at + WATCH_MAX_DRAIN));
        let next_event = skipped_at + WATCH_MAX_DRAIN;
        watch.on_event(next_event);
        assert!(!watch.is_due(next_event));
        assert!(watch.is_due(next_event + WATCH_DEBOUNCE));
        assert!(!watch.should_reload(next_event + WATCH_DEBOUNCE, || stamp(1)));

        let mut watch = ReloadWatch::new(WakeMode::Poll, stamp(1), now);
        let skipped_at = now + WATCH_HEARTBEAT;
        assert!(watch.is_due(skipped_at));
        watch.skip(skipped_at);
        assert!(!watch.is_due(skipped_at));
        assert!(!watch.is_due(skipped_at + WATCH_HEARTBEAT - Duration::from_nanos(1)));
        assert!(watch.should_reload(skipped_at + WATCH_HEARTBEAT, || stamp(2)));
    }

    #[test]
    fn debounce_fires_after_quiet_period() {
        let now = Instant::now();
        let mut watch = ReloadWatch::new(WakeMode::Events, stamp(1), now);
        watch.on_event(now);
        assert!(
            !watch.should_reload(now + WATCH_DEBOUNCE - Duration::from_nanos(1), || panic!(
                "stamp read before quiet period"
            ))
        );
        assert!(watch.should_reload(now + WATCH_DEBOUNCE, || stamp(2)));
        assert!(!watch.should_reload(now + WATCH_MAX_DRAIN, || panic!(
            "pending event was not cleared"
        )));
    }

    #[test]
    fn debounce_is_capped_by_max_drain() {
        let now = Instant::now();
        let mut watch = ReloadWatch::new(WakeMode::Events, stamp(1), now);
        watch.on_event(now);
        for elapsed in [90, 180, 270, 360, 450] {
            let event_time = now + Duration::from_millis(elapsed);
            watch.on_event(event_time);
            assert!(!watch.should_reload(event_time, || panic!("not due yet")));
        }
        assert!(watch.should_reload(now + WATCH_MAX_DRAIN, || stamp(2)));
    }

    #[test]
    fn unchanged_stamp_skips_reload() {
        let now = Instant::now();
        let mut watch = ReloadWatch::new(WakeMode::Events, stamp(1), now);
        watch.on_event(now);
        assert!(!watch.should_reload(now + WATCH_DEBOUNCE, || stamp(1)));
        assert!(!watch.should_reload(now + WATCH_MAX_DRAIN, || panic!(
            "unchanged event was not cleared"
        )));
        watch.on_event(now + WATCH_MAX_DRAIN);
        assert!(watch.should_reload(now + WATCH_MAX_DRAIN + WATCH_DEBOUNCE, || stamp(2)));
        watch.on_event(now + WATCH_MAX_DRAIN + WATCH_DEBOUNCE);
        assert!(!watch.should_reload(now + WATCH_MAX_DRAIN + WATCH_DEBOUNCE * 2, || stamp(2)));
    }

    #[test]
    fn poll_mode_reloads_on_stamp_change() {
        let now = Instant::now();
        let mut watch = ReloadWatch::new(WakeMode::Poll, stamp(1), now);
        assert!(
            !watch.should_reload(now + WATCH_HEARTBEAT - Duration::from_nanos(1), || panic!(
                "heartbeat not due"
            ))
        );
        assert!(!watch.should_reload(now + WATCH_HEARTBEAT, || stamp(1)));
        assert!(watch.should_reload(now + WATCH_HEARTBEAT * 2, || stamp(2)));
        assert!(!watch.should_reload(now + WATCH_HEARTBEAT * 3, || stamp(2)));
    }
}
