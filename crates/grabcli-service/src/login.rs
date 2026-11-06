//! Logins in progress: one driver thread per login, one browser step at a time (start with the phone → a code or
//! the app's approval → done). The browser polls [`Logins::get`]; a login
//! nobody polled for [`Timing::abandon_ms`] is cancelled, as is one the
//! person cancels. A login's id is minted here and bound to the person
//! and the org that started it: another's is "not found".
//!
//! `person` and `org` are the embedder's plain keys: who is logging in
//! (one person may run several logins) and which Grab account it connects
//! (one login at a time per org, and a code-send budget per org).
//!
//! The phone and the code are never kept: the phone goes straight to the
//! driver thread, the code waits in memory only until the driver takes it,
//! and neither is logged.

use alloc::sync::Arc;
use core::time::Duration;
use std::collections::HashMap;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use super::api::{LoginDriver, Step};
use crate::{Log, Secret};

/// The clocks and limits (the tests shrink them).
#[derive(Debug, Clone)]
pub struct Timing {
    /// A login not polled for this long is cancelled.
    pub abandon_ms: i64,
    /// A finished login stays visible this long.
    pub keep_ms: i64,
    /// One wait for the app's approval.
    pub approval_wait: Duration,
    /// How long a login may wait for the app at all.
    pub approval_ms: i64,
    /// Between two code sends of one org.
    pub send_cooldown_s: i64,
    /// Code sends per org per hour.
    pub sends_per_hour: usize,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            abandon_ms: 120_000,
            keep_ms: 600_000,
            approval_wait: Duration::from_secs(5),
            approval_ms: 300_000,
            send_cooldown_s: 60,
            sends_per_hour: 3,
        }
    }
}

/// Where a login is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    /// The browser is on its way to Grab's login.
    Starting,
    /// Enter the code Grab sent (`channel`).
    Code {
        /// How Grab sent it (sms, whatsapp, ...).
        channel: String,
    },
    /// Approve it in the Grab app.
    Approve,
    /// Connected.
    Done {
        /// How the account is shown: for the real backend, grabcli's masked
        /// phone (`+1••••••••00`: the last two digits shown).
        display_name: String,
    },
    /// It ended without a session.
    Failed {
        /// Why (no secrets: the login's phone is redacted from it).
        message: String,
    },
    /// Its person cancelled it.
    Cancelled,
    /// Nobody polled it.
    Expired,
}

impl View {
    /// Whether it ended (done, failed, cancelled or expired).
    pub fn finished(&self) -> bool {
        matches!(
            self,
            Self::Done { .. } | Self::Failed { .. } | Self::Cancelled | Self::Expired
        )
    }

    /// Its name (`starting`, `code`, `approve`, `done`, `failed`,
    /// `cancelled`, `expired`).
    pub fn state(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Code { .. } => "code",
            Self::Approve => "approve",
            Self::Done { .. } => "done",
            Self::Failed { .. } => "failed",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }
}

/// A login as its person sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The login's id (for [`Logins::get`], `submit`, `cancel`).
    pub id: String,
    /// The account it connects.
    pub org: String,
    /// Where it is.
    pub view: View,
    /// Bumped on every change of `view` (a poller's cue to redraw).
    pub revision: u32,
    /// A code submitted, the browser on it.
    pub busy: bool,
}

/// Why a call was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoginError {
    /// No such login (or another person's or org's).
    #[error("{0}")]
    NotFound(&'static str),
    /// A bad phone or code (or no thread to drive it).
    #[error("{0}")]
    Invalid(String),
    /// Not now: one in progress already, or not asking for a code.
    #[error("{0}")]
    State(&'static str),
    /// The org's send budget (a cooldown, or the hourly cap).
    #[error("{0}")]
    Budget(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Cancel,
    Abandon,
}

#[derive(Debug)]
struct State {
    view: View,
    revision: u32,
    last_poll: i64,
    finished_at: Option<i64>,
    stop: Option<Stop>,
    code: Option<Secret>,
    busy: bool,
}

#[derive(Debug)]
struct Pending {
    id: String,
    person: String,
    org: String,
    st: Mutex<State>,
    cv: Condvar,
}

impl Pending {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn snapshot(&self) -> Snapshot {
        let st = self.lock();
        Snapshot {
            id: self.id.clone(),
            org: self.org.clone(),
            view: st.view.clone(),
            revision: st.revision,
            busy: st.busy,
        }
    }
}

/// What a finished login hands back: the session to keep and the name.
/// Its `Debug` never shows the session.
pub struct LoggedIn {
    /// The account it connected.
    pub org: String,
    /// Who logged in.
    pub person: String,
    /// grabcli's `Session` as JSON (secret: keep it sealed).
    pub session: serde_json::Value,
    /// How the account is shown: for the real backend, grabcli's masked
    /// phone (`+1••••••••00`: the last two digits shown).
    pub display_name: String,
    /// When the SSO login lapses (unix seconds), if known.
    pub sso_expires_at: Option<i64>,
}

impl core::fmt::Debug for LoggedIn {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LoggedIn")
            .field("org", &self.org)
            .field("person", &self.person)
            .field("session", &crate::Redacted)
            .field("display_name", &self.display_name)
            .field("sso_expires_at", &self.sso_expires_at)
            .finish()
    }
}

/// `text` without `phone` in any of the forms an error may echo it: as
/// given, its digits (with or without the `+`), and its digits after a
/// one- to three-digit country code.
fn scrub_phone(phone: &Secret, text: &str) -> String {
    let given = phone.expose().trim();
    let digits: String = given.chars().filter(char::is_ascii_digit).collect();
    let mut forms = vec![given.to_owned(), digits.clone()];
    for cc in 1..=3 {
        if digits.len() >= cc + 7 {
            forms.push(digits[cc..].to_owned());
        }
    }
    // Longest first, so a shorter form doesn't split a longer one.
    forms.sort_by_key(|f| core::cmp::Reverse(f.len()));
    forms
        .iter()
        .filter(|f| !f.is_empty())
        .fold(text.to_owned(), |t, f| Secret::new(f.as_str()).redact(&t))
}

/// Where a done login's session goes (the embedder keeps it for the org);
/// an error fails the login.
pub type OnDone = Arc<dyn Fn(LoggedIn) -> Result<(), String> + Send + Sync>;

/// The logins in progress.
pub struct Logins {
    pending: Mutex<HashMap<String, Arc<Pending>>>,
    /// Code sends per org (unix seconds), for the budget.
    sends: Mutex<HashMap<String, Vec<i64>>>,
    /// The clocks and limits.
    pub timing: Timing,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    log: Option<Log>,
}

impl Default for Logins {
    fn default() -> Self {
        Self::new(Timing::default(), Arc::new(crate::util::now_ms))
    }
}

impl Logins {
    /// Logins on `timing`, with `clock` (unix milliseconds) as now.
    pub fn new(timing: Timing, clock: Arc<dyn Fn() -> i64 + Send + Sync>) -> Self {
        Self {
            pending: Mutex::default(),
            sends: Mutex::default(),
            timing,
            clock,
            log: None,
        }
    }

    /// Send diagnostics (a login connected, failed, abandoned; never a
    /// phone or a code) to `log`.
    #[must_use]
    pub fn with_log(mut self, log: Log) -> Self {
        self.log = Some(log);
        self
    }

    fn log(&self, line: &str) {
        if let Some(l) = &self.log {
            l(line);
        }
    }

    fn now(&self) -> i64 {
        (self.clock)()
    }

    fn table(&self) -> MutexGuard<'_, HashMap<String, Arc<Pending>>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn mine(&self, person: &str, org: &str, id: &str) -> Result<Arc<Pending>, LoginError> {
        self.table()
            .get(id)
            .filter(|p| p.person == person && p.org == org)
            .cloned()
            .ok_or(LoginError::NotFound("no such login"))
    }

    /// `person`'s logins of `org` in progress (and those just finished).
    pub fn of(&self, person: &str, org: &str) -> Vec<Snapshot> {
        self.reap();
        let mut v: Vec<Snapshot> = self
            .table()
            .values()
            .filter(|p| p.person == person && p.org == org)
            .map(|p| p.snapshot())
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    /// Take one send from `org`'s budget (60 s apart, 3 an hour).
    fn take_send(&self, org: &str) -> Result<(), LoginError> {
        let now = self.now() / 1000;
        let mut all = self.sends.lock().unwrap_or_else(PoisonError::into_inner);
        let sends = all.entry(org.to_owned()).or_default();
        sends.retain(|t| now - t < 3600);
        if let Some(last) = sends.last()
            && now - last < self.timing.send_cooldown_s
        {
            return Err(LoginError::Budget(format!(
                "Grab just sent a code: wait {} s before asking again",
                self.timing.send_cooldown_s - (now - last)
            )));
        }
        if sends.len() >= self.timing.sends_per_hour {
            return Err(LoginError::Budget(
                "too many code sends in the last hour: try again later".into(),
            ));
        }
        sends.push(now);
        Ok(())
    }

    /// A new login of `org` by `person` with `phone` on `driver`: its
    /// thread drives it; `done` keeps the session. One at a time per org.
    ///
    /// # Errors
    /// A bad phone, one in progress already, the send budget.
    pub fn start(
        self: &Arc<Self>,
        person: &str,
        org: &str,
        phone: Secret,
        driver: Box<dyn LoginDriver>,
        done: OnDone,
    ) -> Result<Snapshot, LoginError> {
        self.reap();
        let digits = phone.expose().chars().filter(char::is_ascii_digit).count();
        if !phone.expose().trim().starts_with('+') || !(8..=15).contains(&digits) {
            return Err(LoginError::Invalid(
                "the phone number: in international form, + and the country code first".into(),
            ));
        }
        // One critical section: the check, the send and the insert, so two
        // starts for one org can't both pass.
        let mut table = self.table();
        if table
            .values()
            .any(|p| p.org == org && !p.lock().view.finished())
        {
            return Err(LoginError::State(
                "a Grab login is already in progress for this org",
            ));
        }
        self.take_send(org)?;
        let now = self.now();
        let p = Arc::new(Pending {
            id: crate::util::new_id(),
            person: person.to_owned(),
            org: org.to_owned(),
            st: Mutex::new(State {
                view: View::Starting,
                revision: 0,
                last_poll: now,
                finished_at: None,
                stop: None,
                code: None,
                busy: false,
            }),
            cv: Condvar::new(),
        });
        table.insert(p.id.clone(), Arc::clone(&p));
        drop(table);
        let (hub, p2) = (Arc::clone(self), Arc::clone(&p));
        std::thread::Builder::new()
            .name("grab-login".into())
            .spawn(move || {
                let mut driver = driver;
                let r = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
                    hub.drive(&p2, driver.as_mut(), phone, &done);
                }));
                driver.cancel();
                if r.is_err() {
                    hub.finish(
                        &p2,
                        View::Failed {
                            message: "the login broke (logged)".into(),
                        },
                    );
                    hub.log(&format!("org {}: the login driver panicked", p2.org));
                }
            })
            .map_err(|e| {
                self.finish(
                    &p,
                    View::Failed {
                        message: "the login couldn't start".into(),
                    },
                );
                LoginError::Invalid(format!("a thread: {e}"))
            })?;
        Ok(p.snapshot())
    }

    /// Where a login is (it counts as a poll).
    ///
    /// # Errors
    /// Not found (or another's).
    pub fn get(&self, person: &str, org: &str, id: &str) -> Result<Snapshot, LoginError> {
        self.reap();
        let p = self.mine(person, org, id)?;
        p.lock().last_poll = self.now();
        Ok(p.snapshot())
    }

    /// The code Grab sent.
    ///
    /// # Errors
    /// Not found, not asking for one, already submitted, empty.
    pub fn submit(
        &self,
        person: &str,
        org: &str,
        id: &str,
        code: Secret,
    ) -> Result<Snapshot, LoginError> {
        let p = self.mine(person, org, id)?;
        let c = code.expose().trim();
        if c.is_empty() || c.len() > 12 || !c.chars().all(|x| x.is_ascii_alphanumeric()) {
            return Err(LoginError::Invalid("the code: the digits Grab sent".into()));
        }
        {
            let mut st = p.lock();
            if !matches!(st.view, View::Code { .. }) {
                return Err(LoginError::State("this login isn't asking for a code"));
            }
            if st.busy || st.code.is_some() {
                return Err(LoginError::State("already submitted"));
            }
            st.code = Some(Secret::new(c));
            st.busy = true;
            st.last_poll = self.now();
        }
        p.cv.notify_all();
        Ok(p.snapshot())
    }

    /// Stop a login.
    ///
    /// # Errors
    /// Not found (or another's).
    pub fn cancel(&self, person: &str, org: &str, id: &str) -> Result<Snapshot, LoginError> {
        let p = self.mine(person, org, id)?;
        stop(&p, Stop::Cancel);
        Ok(p.snapshot())
    }

    /// Cancel the logins nobody polled; forget those finished a while ago.
    pub fn reap(&self) {
        let now = self.now();
        let all: Vec<Arc<Pending>> = self.table().values().cloned().collect();
        let mut gone = Vec::new();
        for p in all {
            let (abandoned, old) = {
                let st = p.lock();
                (
                    st.stop.is_none()
                        && !st.view.finished()
                        && now - st.last_poll > self.timing.abandon_ms,
                    st.finished_at
                        .is_some_and(|t| now - t > self.timing.keep_ms),
                )
            };
            if abandoned {
                self.log(&format!("org {}: a login was abandoned, cancelling", p.org));
                stop(&p, Stop::Abandon);
            }
            if old {
                gone.push(p.id.clone());
            }
        }
        if !gone.is_empty() {
            let mut t = self.table();
            for id in gone {
                t.remove(&id);
            }
        }
    }

    fn drive(&self, p: &Pending, d: &mut dyn LoginDriver, phone: Secret, done: &OnDone) {
        // The phone stays only to scrub it from a failure's message.
        let mut step = d.start(&phone);
        let approve_until = self.now() + self.timing.approval_ms;
        loop {
            if self.stopped(p) {
                return;
            }
            step = match step {
                Step::Done {
                    session,
                    display_name,
                    sso_expires_at,
                } => {
                    let r = done(LoggedIn {
                        org: p.org.clone(),
                        person: p.person.clone(),
                        session,
                        display_name: display_name.clone(),
                        sso_expires_at,
                    });
                    return match r {
                        Ok(()) => {
                            self.log(&format!("org {}: connected", p.org));
                            self.finish(p, View::Done { display_name });
                        }
                        Err(e) => {
                            self.log(&format!("org {}: keeping the session: {e}", p.org));
                            self.finish(
                                p,
                                View::Failed {
                                    message: "couldn't keep the session (logged)".into(),
                                },
                            );
                        }
                    };
                }
                Step::Failed(message) => {
                    let message = scrub_phone(&phone, &message);
                    self.log(&format!("org {}: the login failed: {message}", p.org));
                    return self.finish(p, View::Failed { message });
                }
                Step::AwaitingApproval => {
                    if self.now() > approve_until {
                        return self.finish(
                            p,
                            View::Failed {
                                message: "nobody approved it in the Grab app in time".into(),
                            },
                        );
                    }
                    Self::publish(p, View::Approve);
                    self.abandon_if_idle(p);
                    if self.stopped(p) {
                        return;
                    }
                    d.wait(self.timing.approval_wait)
                }
                Step::AwaitingCode { channel } => {
                    Self::publish(p, View::Code { channel });
                    let code = loop {
                        let st = p.lock();
                        let (mut st, _) =
                            p.cv.wait_timeout_while(st, Duration::from_secs(1), |s| {
                                s.code.is_none() && s.stop.is_none()
                            })
                            .unwrap_or_else(PoisonError::into_inner);
                        if st.code.is_some() || st.stop.is_some() {
                            break st.code.take();
                        }
                        drop(st);
                        self.abandon_if_idle(p);
                    };
                    if self.stopped(p) {
                        return;
                    }
                    let Some(code) = code else { return };
                    d.submit_code(&code)
                }
            };
        }
    }

    /// Nobody polled it for a while: stop it (the driver's own check, so a
    /// login ends without a poll to reap it).
    fn abandon_if_idle(&self, p: &Pending) {
        let idle = {
            let st = p.lock();
            st.stop.is_none() && self.now() - st.last_poll > self.timing.abandon_ms
        };
        if idle {
            self.log(&format!("org {}: a login was abandoned, cancelling", p.org));
            stop(p, Stop::Abandon);
        }
    }

    fn publish(p: &Pending, view: View) {
        let mut st = p.lock();
        if st.view.finished() {
            return;
        }
        if st.view != view {
            st.view = view;
            st.revision += 1;
        }
        st.busy = false;
    }

    fn finish(&self, p: &Pending, view: View) {
        let mut st = p.lock();
        if st.view.finished() {
            return;
        }
        st.view = view;
        st.revision += 1;
        st.busy = false;
        st.code = None;
        st.finished_at = Some(self.now());
        drop(st);
        p.cv.notify_all();
    }

    fn stopped(&self, p: &Pending) -> bool {
        let Some(why) = p.lock().stop else {
            return false;
        };
        self.finish(
            p,
            match why {
                Stop::Cancel => View::Cancelled,
                Stop::Abandon => View::Expired,
            },
        );
        true
    }
}

fn stop(p: &Pending, why: Stop) {
    {
        let mut st = p.lock();
        if st.view.finished() || st.stop.is_some() {
            return;
        }
        st.stop = Some(why);
    }
    p.cv.notify_all();
}
