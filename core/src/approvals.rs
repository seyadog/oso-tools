//! The person's say over what an AI agent asks to do. A request waits here until the window answers (or the time
//! runs out, which is a no); with no window to answer, it is a no at once.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// What the window shows. It never carries a secret: the command is what the agent typed, which is what the person
/// is approving.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ApprovalRequest {
    pub id: u64,
    pub pane: u64,
    /// "Claude in example-api, pane 2": who asks, from where.
    pub label: String,
    pub agent: String,
    /// `run`, `sudo`, `run_with` or `tunnel`.
    pub action: String,
    pub host: String,
    pub command: String,
    pub production: bool,
    /// Whether "always for read-only commands on this host" makes sense (not on production, not for a write).
    pub can_remember: bool,
}

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Once,
    /// Once, and from now on read-only commands on this host go through without asking.
    AlwaysRead,
    Deny,
    /// The person will run it themselves in a terminal: the agent is told no.
    Terminal,
}

type Notify = Arc<dyn Fn(ApprovalRequest) + Send + Sync>;

#[derive(Default)]
struct Inner {
    next: u64,
    waiting: HashMap<u64, Option<Decision>>,
    notify: Option<Notify>,
}

pub struct Approvals {
    inner: Mutex<Inner>,
    cv: Condvar,
}

/// More than this waiting at once is an agent in a loop, not a person being asked.
const MAX_WAITING: usize = 12;

impl Approvals {
    pub fn new() -> Arc<Approvals> {
        Arc::new(Approvals { inner: Mutex::new(Inner::default()), cv: Condvar::new() })
    }

    /// Where requests are shown (the window).
    pub fn on_request(&self, f: Notify) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).notify = Some(f);
    }

    /// Shows the request and waits for the answer. No window, too many waiting, or no answer in `wait`: no.
    pub fn ask(&self, mut req: ApprovalRequest, wait: Duration) -> Decision {
        let (id, notify) = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let Some(notify) = g.notify.clone() else { return Decision::Deny };
            if g.waiting.len() >= MAX_WAITING {
                return Decision::Deny;
            }
            g.next += 1;
            let id = g.next;
            g.waiting.insert(id, None);
            (id, notify)
        };
        req.id = id;
        notify(req);
        let end = Instant::now() + wait;
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(Some(d)) = g.waiting.get(&id) {
                let d = *d;
                g.waiting.remove(&id);
                return d;
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                g.waiting.remove(&id);
                return Decision::Deny;
            }
            g = self.cv.wait_timeout(g, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// The window's answer. `false` if nobody was waiting for it any more.
    pub fn answer(&self, id: u64, d: Decision) -> bool {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match g.waiting.get_mut(&id) {
            Some(slot @ None) => {
                *slot = Some(d);
                self.cv.notify_all();
                true
            }
            _ => false,
        }
    }

    pub fn waiting(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).waiting.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> ApprovalRequest {
        ApprovalRequest { id: 0, pane: 2, label: "Claude in example-api, pane 2".into(), agent: "claude".into(), action: "run".into(), host: "example".into(), command: "df -h".into(), production: false, can_remember: true }
    }

    #[test]
    fn a_request_waits_for_the_answer_and_names_who_asks() {
        let a = Approvals::new();
        let seen: Arc<Mutex<Vec<ApprovalRequest>>> = Arc::default();
        let (s, b) = (Arc::clone(&seen), Arc::clone(&a));
        a.on_request(Arc::new(move |r| {
            s.lock().unwrap().push(r.clone());
            let b = Arc::clone(&b);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(30));
                assert!(b.answer(r.id, Decision::Once));
            });
        }));
        assert_eq!(a.ask(req(), Duration::from_secs(5)), Decision::Once);
        let r = seen.lock().unwrap()[0].clone();
        assert_eq!((r.label.as_str(), r.command.as_str(), r.id), ("Claude in example-api, pane 2", "df -h", 1));
        assert_eq!(a.waiting(), 0);
    }

    #[test]
    fn without_a_window_or_an_answer_it_is_a_no() {
        let a = Approvals::new();
        assert_eq!(a.ask(req(), Duration::from_secs(5)), Decision::Deny, "no window to show it");
        a.on_request(Arc::new(|_| {}));
        let t = Instant::now();
        assert_eq!(a.ask(req(), Duration::from_millis(80)), Decision::Deny, "nobody answered");
        assert!(t.elapsed() >= Duration::from_millis(70) && a.waiting() == 0);
        assert!(!a.answer(99, Decision::Once), "nobody is waiting for that one");
    }

    #[test]
    fn an_answer_counts_once_and_too_many_waiting_are_refused() {
        let a = Approvals::new();
        let b = Arc::clone(&a);
        a.on_request(Arc::new(move |r| {
            let b = Arc::clone(&b);
            std::thread::spawn(move || {
                assert!(b.answer(r.id, Decision::Deny));
                assert!(!b.answer(r.id, Decision::Once), "the first answer is the one");
            });
        }));
        assert_eq!(a.ask(req(), Duration::from_secs(5)), Decision::Deny);
        let quiet = Approvals::new();
        quiet.on_request(Arc::new(|_| {}));
        let handles: Vec<_> = (0..MAX_WAITING).map(|_| {
            let q = Arc::clone(&quiet);
            std::thread::spawn(move || q.ask(req(), Duration::from_millis(400)))
        }).collect();
        std::thread::sleep(Duration::from_millis(100));
        let t = Instant::now();
        assert_eq!(quiet.ask(req(), Duration::from_secs(5)), Decision::Deny);
        assert!(t.elapsed() < Duration::from_secs(1), "refused at once, not after the wait");
        for h in handles {
            h.join().unwrap();
        }
    }
}
