//! Throttling failed authentication (KAIROS-T-0202, KAIROS-I-0018).
//!
//! Kairos had no rate limiting anywhere. That was survivable while every
//! credential was either a 32-byte random API key or a signed JWT — guessing
//! those is infeasible regardless — but KAIROS-I-0018 adds a password endpoint,
//! and a password endpoint without a throttle is a brute-force target.
//!
//! # Scope
//!
//! Failed **authentication** only, not a general-purpose request limiter. A
//! limiter for every endpoint is a bigger product decision than this needs, and
//! pretending this is one would be worse than saying it is not. The mechanism
//! could be generalised later; nothing here assumes it will be.
//!
//! # Where the state lives, and what that costs
//!
//! In process, in a `Mutex<HashMap>`. The alternative was PostgreSQL, and the
//! trade was:
//!
//! - **In process** costs no round trip and survives nothing. State is
//!   **per replica**, so a deployment behind an HPA multiplies the allowance by
//!   the replica count, and a restart forgets every lockout.
//! - **In PostgreSQL** is shared and durable, and puts a write on the failure
//!   path of an endpoint that is being attacked — the moment you least want
//!   extra writes.
//!
//! In process wins for KAIROS-A-0013's one-binary shape and for the small
//! deployments KAIROS-I-0018 targets. The replica-count caveat is real and is
//! documented for operators rather than hidden here. [`AuthThrottle`] is the only
//! type callers touch, so a shared implementation can replace the internals
//! without changing a handler.
//!
//! # Time is a parameter
//!
//! Every method takes `now`. Decay is arithmetic on instants, and a test that has
//! to `sleep` to check a lockout expires is a slow test that will one day be
//! flaky. Callers pass `Instant::now()`; tests pass whatever they want.

use axum::extract::{ConnectInfo, Request};
use axum::http::HeaderValue;
use axum::http::header::RETRY_AFTER;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::ApiError;

/// What is being counted against. Both are tracked, deliberately.
///
/// Per-identity alone lets an attacker spray one attempt each at a thousand
/// accounts and never trip a limit. Per-source alone lets a stranger lock a known
/// account out of a service by failing against it, and is meaningless behind a
/// proxy where every request shares one address. Neither is sufficient; together
/// they cover the realistic attacks.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Subject {
    /// The identity being attempted — an email, or an API key's tenant.
    Identity(String),
    /// Where the attempt came from.
    Source(IpAddr),
}

impl Subject {
    /// The metric/log label for this kind of subject. The kind is safe to record
    /// where the value is not.
    pub fn kind(&self) -> &'static str {
        match self {
            Subject::Identity(_) => "identity",
            Subject::Source(_) => "source",
        }
    }
}

/// Thresholds. Defaults chosen to be invisible to a person who mistypes and
/// expensive for a script.
#[derive(Debug, Clone, Copy)]
pub struct ThrottleConfig {
    /// Failures within `window` before the subject is locked out.
    pub max_failures: u32,
    /// How long failures accumulate. Older ones are forgotten.
    pub window: Duration,
    /// How long a lockout lasts once tripped.
    pub lockout: Duration,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            // Five is comfortably above a human's mistyping and far below what a
            // script needs to be useful.
            max_failures: 5,
            window: Duration::from_secs(300),
            // A minute, and DELIBERATELY SHORT. A long lockout is a
            // denial-of-service an attacker can aim at a known account, and it
            // turns one person's typo into a support request. Short and repeatable
            // beats long and punitive: five more failures locks them out again.
            lockout: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    failures: u32,
    /// When the current window started. Used to decay, not to expire the entry.
    window_started: Instant,
    locked_until: Option<Instant>,
}

/// The throttle. Cheap to share behind an `Arc`; interior-mutable because
/// recording happens on the request path.
#[derive(Debug)]
pub struct AuthThrottle {
    config: ThrottleConfig,
    buckets: Mutex<HashMap<Subject, Bucket>>,
}

/// A subject is locked out, and for how much longer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockedOut {
    /// Seconds until the subject may try again — the `Retry-After` value.
    pub retry_after_secs: u64,
}

impl AuthThrottle {
    pub fn new(config: ThrottleConfig) -> Self {
        Self {
            config,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// May these subjects attempt authentication?
    ///
    /// Checked BEFORE verifying a credential, so a locked-out attacker does not
    /// get the argon2 work done on their behalf — which matters because that work
    /// is the expensive thing they would otherwise be making the server do.
    pub fn check(&self, subjects: &[Subject], now: Instant) -> Result<(), LockedOut> {
        let buckets = self.buckets.lock().expect("throttle mutex");
        let longest = subjects
            .iter()
            .filter_map(|s| buckets.get(s))
            .filter_map(|b| b.locked_until)
            .filter(|until| *until > now)
            .max();
        match longest {
            // Rounded UP, so a caller that waits exactly this long is past it
            // rather than one millisecond short and refused again.
            Some(until) => Err(LockedOut {
                retry_after_secs: (until - now).as_secs() + 1,
            }),
            None => Ok(()),
        }
    }

    /// Record a failed attempt, and lock out any subject that has now had too
    /// many. Returns the lockout if one was tripped, for logging.
    pub fn record_failure(&self, subjects: &[Subject], now: Instant) -> Option<LockedOut> {
        let mut buckets = self.buckets.lock().expect("throttle mutex");
        let mut tripped = None;
        for subject in subjects {
            let bucket = buckets.entry(subject.clone()).or_insert(Bucket {
                failures: 0,
                window_started: now,
                locked_until: None,
            });
            // Decay: a window that has elapsed starts over, so yesterday's typo
            // does not combine with today's.
            if now.duration_since(bucket.window_started) >= self.config.window {
                bucket.failures = 0;
                bucket.window_started = now;
                bucket.locked_until = None;
            }
            bucket.failures += 1;
            if bucket.failures >= self.config.max_failures {
                let until = now + self.config.lockout;
                bucket.locked_until = Some(until);
                // Counting restarts, so the next burst has to earn its own
                // lockout rather than inheriting this one.
                bucket.failures = 0;
                bucket.window_started = now;
                tripped = Some(LockedOut {
                    retry_after_secs: self.config.lockout.as_secs(),
                });
            }
        }
        tripped
    }

    /// Record a success, clearing the subjects' counters.
    ///
    /// Only the counters — a live lockout is NOT cleared. Otherwise an attacker
    /// who locks an account out and then authenticates as themselves from the
    /// same address would lift it.
    pub fn record_success(&self, subjects: &[Subject], now: Instant) {
        let mut buckets = self.buckets.lock().expect("throttle mutex");
        for subject in subjects {
            if let Some(bucket) = buckets.get_mut(subject)
                && bucket.locked_until.is_none_or(|until| until <= now)
            {
                bucket.failures = 0;
                bucket.window_started = now;
                bucket.locked_until = None;
            }
        }
    }

    /// Drop entries that can no longer affect a decision.
    ///
    /// Without this the map grows once per distinct attempted identity, which an
    /// attacker controls — so it is a memory-exhaustion vector, not housekeeping.
    /// Called opportunistically rather than on a timer; there is no background
    /// task to own.
    pub fn evict_expired(&self, now: Instant) -> usize {
        let mut buckets = self.buckets.lock().expect("throttle mutex");
        let before = buckets.len();
        buckets.retain(|_, b| {
            let locked = b.locked_until.is_some_and(|until| until > now);
            let counting = now.duration_since(b.window_started) < self.config.window;
            locked || counting
        });
        before - buckets.len()
    }

    /// How many subjects are being tracked (for the metric, and for tests).
    pub fn tracked(&self) -> usize {
        self.buckets.lock().expect("throttle mutex").len()
    }
}

/// Build the throttle this deployment's configuration asks for, or `None` when
/// `KAIROS_AUTH_MAX_FAILURES` is 0 and throttling is off.
pub fn from_config(config: &crate::config::AppConfig) -> Option<AuthThrottle> {
    (config.auth_max_failures > 0).then(|| {
        AuthThrottle::new(ThrottleConfig {
            max_failures: config.auth_max_failures,
            window: Duration::from_secs(config.auth_failure_window_secs),
            lockout: Duration::from_secs(config.auth_lockout_secs),
        })
    })
}

/// The client address to count against, or `None` when nothing trustworthy is
/// available.
///
/// # What is trusted, and why so little
///
/// `X-Forwarded-For` is read **only** when `KAIROS_TRUSTED_PROXY` is on, because
/// the header is caller-supplied: on a directly exposed server, trusting it gives
/// an attacker a new identity per request, which does not weaken a source-based
/// throttle so much as delete it.
///
/// When it is trusted, the **LAST** element is used, not the first. A proxy
/// APPENDS the peer it saw, so the list reads
/// `<whatever the client sent>, <the address the proxy observed>`. The first
/// element is the one under the caller's control; reading it is the classic form
/// of this bug.
///
/// With the switch on and no header present, the answer is `None` rather than the
/// socket peer. Behind a proxy the peer IS the proxy, so counting it would pool
/// every client into one bucket and let one clumsy client lock out the world.
///
/// `None` is also what in-process tests see, since `Router::oneshot` inserts no
/// [`ConnectInfo`]. That is correct rather than convenient: with no address to
/// attribute, there is no source to throttle.
pub fn client_addr(req: &Request, trusted_proxy: bool) -> Option<IpAddr> {
    if trusted_proxy {
        return req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|list| {
                list.rsplit(',')
                    .map(str::trim)
                    .find(|s| !s.is_empty())
                    .and_then(|s| s.parse().ok())
            });
    }
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip())
}

/// The 429 for a locked-out caller, with `Retry-After`.
///
/// A [`Response`] rather than an [`ApiError`], because `ApiError` carries no
/// headers and `Retry-After` is the one thing a well-behaved client needs from
/// this reply. Callers that return `Result<Response, ApiError>` hand this back as
/// `Ok` — unusual enough to say out loud, which is why it lives here and not
/// inline at each call site.
pub fn locked_out_response(locked: LockedOut) -> Response {
    let mut response = ApiError::too_many_requests(locked.retry_after_secs).into_response();
    if let Ok(value) = HeaderValue::from_str(&locked.retry_after_secs.to_string()) {
        response.headers_mut().insert(RETRY_AFTER, value);
    }
    response
}

/// Above this many tracked subjects, a recorded failure also sweeps the expired
/// ones. Opportunistic, so there is no background task to own and no timer to
/// keep alive in a test.
const EVICT_ABOVE: usize = 1024;

/// One authentication attempt, from the throttle's point of view.
///
/// Wraps the three-step dance — refuse if locked out, then report success or
/// failure — so a call site cannot accidentally do two of the three. Also absorbs
/// "this deployment has throttling off" and "there is no address to attribute",
/// which otherwise spread `if let Some` across every auth path.
pub struct Attempt<'a> {
    throttle: Option<&'a AuthThrottle>,
    metrics: &'a crate::metrics::Metrics,
    subjects: Vec<Subject>,
    now: Instant,
}

impl<'a> Attempt<'a> {
    /// Begin an attempt against `identity` (the thing being authenticated *as*,
    /// when the caller has one worth naming) from `source`.
    ///
    /// Either may be absent. With both absent the attempt is unthrottled, because
    /// there is nothing to count against — which is the honest outcome rather than
    /// a hole: an attempt that cannot be attributed cannot be rationed either.
    pub fn begin(
        state: &'a crate::app::AppState,
        source: Option<IpAddr>,
        identity: Option<String>,
    ) -> Self {
        let mut subjects = Vec::with_capacity(2);
        if let Some(identity) = identity {
            subjects.push(Subject::Identity(identity));
        }
        if let Some(source) = source {
            subjects.push(Subject::Source(source));
        }
        Self {
            throttle: state.throttle.as_deref(),
            metrics: &state.metrics,
            subjects,
            now: Instant::now(),
        }
    }

    /// The 429 to return instead of checking the credential, if this caller is
    /// locked out.
    ///
    /// Called BEFORE the credential is verified, so the expensive work — an argon2
    /// verification, or two database round trips for an API key — is never done on
    /// a locked-out caller's behalf.
    pub fn refuse_if_locked_out(&self) -> Option<Response> {
        let throttle = self.throttle?;
        match throttle.check(&self.subjects, self.now) {
            Ok(()) => None,
            Err(locked) => {
                tracing::debug!(
                    retry_after_secs = locked.retry_after_secs,
                    "refusing a locked-out authentication attempt"
                );
                Some(locked_out_response(locked))
            }
        }
    }

    /// Report that the credential did not verify.
    pub fn failed(&self) {
        let Some(throttle) = self.throttle else {
            return;
        };
        if let Some(locked) = throttle.record_failure(&self.subjects, self.now) {
            // WARN, not INFO: a lockout is the one event here an operator wants to
            // find in a log. The subject KIND is recorded and the subject VALUE is
            // not — an email or an address in a log line outlives the incident it
            // was gathered for.
            for subject in &self.subjects {
                let kind = subject.kind();
                self.metrics.record_lockout(kind);
                tracing::warn!(
                    subject = kind,
                    retry_after_secs = locked.retry_after_secs,
                    "authentication lockout"
                );
            }
        }
        if throttle.tracked() > EVICT_ABOVE {
            throttle.evict_expired(self.now);
        }
    }

    /// Report that the credential verified, clearing the counters.
    pub fn succeeded(&self) {
        if let Some(throttle) = self.throttle {
            throttle.record_success(&self.subjects, self.now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subjects() -> Vec<Subject> {
        vec![
            Subject::Identity("someone@example.test".into()),
            Subject::Source("203.0.113.9".parse().unwrap()),
        ]
    }

    fn cfg() -> ThrottleConfig {
        ThrottleConfig {
            max_failures: 3,
            window: Duration::from_secs(60),
            lockout: Duration::from_secs(30),
        }
    }

    #[test]
    fn failures_below_the_threshold_do_not_lock_out() {
        let t = AuthThrottle::new(cfg());
        let now = Instant::now();
        let s = subjects();
        assert!(t.record_failure(&s, now).is_none());
        assert!(t.record_failure(&s, now).is_none());
        assert!(t.check(&s, now).is_ok());
    }

    #[test]
    fn the_threshold_locks_out_and_the_lockout_expires() {
        let t = AuthThrottle::new(cfg());
        let start = Instant::now();
        let s = subjects();
        for _ in 0..3 {
            t.record_failure(&s, start);
        }
        let locked = t.check(&s, start).expect_err("should be locked out");
        assert!(locked.retry_after_secs <= 31, "{locked:?}");

        // Still locked one second before expiry, free one second after. This is
        // the decay assertion, and it needs no sleeping.
        assert!(t.check(&s, start + Duration::from_secs(29)).is_err());
        assert!(t.check(&s, start + Duration::from_secs(31)).is_ok());
    }

    #[test]
    fn failures_decay_so_a_slow_typist_is_never_locked_out() {
        // Three failures spread beyond the window must not accumulate — this is
        // the difference between a throttle and a booby trap.
        let t = AuthThrottle::new(cfg());
        let start = Instant::now();
        let s = subjects();
        t.record_failure(&s, start);
        t.record_failure(&s, start + Duration::from_secs(61));
        t.record_failure(&s, start + Duration::from_secs(122));
        assert!(t.check(&s, start + Duration::from_secs(122)).is_ok());
    }

    #[test]
    fn identity_and_source_are_counted_separately() {
        // An attacker spraying one attempt each across many identities from one
        // address must still trip the SOURCE limit, which is the whole reason
        // both are tracked.
        let t = AuthThrottle::new(cfg());
        let now = Instant::now();
        let source = Subject::Source("203.0.113.9".parse().unwrap());
        for i in 0..3 {
            let victim = Subject::Identity(format!("victim{i}@example.test"));
            t.record_failure(&[victim, source.clone()], now);
        }
        // No single identity reached the threshold...
        assert!(
            t.check(&[Subject::Identity("victim0@example.test".into())], now)
                .is_ok()
        );
        // ...but the address did.
        assert!(t.check(&[source], now).is_err());
    }

    #[test]
    fn a_success_clears_the_count_but_not_a_live_lockout() {
        let t = AuthThrottle::new(cfg());
        let now = Instant::now();
        let s = subjects();
        t.record_failure(&s, now);
        t.record_failure(&s, now);
        t.record_success(&s, now);
        // The two failures are forgotten, so three more are needed.
        assert!(t.record_failure(&s, now).is_none());
        assert!(t.record_failure(&s, now).is_none());
        assert!(t.record_failure(&s, now).is_some());

        // And a success during a lockout does not lift it: otherwise an attacker
        // who locked a victim out could clear it by logging in as themselves from
        // the same address.
        t.record_success(&s, now);
        assert!(t.check(&s, now).is_err(), "a live lockout must survive");
    }

    #[test]
    fn eviction_drops_only_what_cannot_matter() {
        // The map is keyed by attacker-supplied identities, so unbounded growth is
        // a memory-exhaustion vector rather than untidiness.
        let t = AuthThrottle::new(cfg());
        let start = Instant::now();
        for i in 0..100 {
            t.record_failure(&[Subject::Identity(format!("a{i}@example.test"))], start);
        }
        assert_eq!(t.tracked(), 100);
        // Inside the window, nothing is droppable.
        assert_eq!(t.evict_expired(start + Duration::from_secs(1)), 0);
        // Past it, all of them are.
        assert_eq!(t.evict_expired(start + Duration::from_secs(61)), 100);
        assert_eq!(t.tracked(), 0);
    }

    #[test]
    fn a_locked_out_subject_is_not_evicted_early() {
        // A lockout OUTLASTING the window is the only case where the two
        // conditions disagree, so it is the only case that tests the `||`.
        let t = AuthThrottle::new(ThrottleConfig {
            max_failures: 3,
            window: Duration::from_secs(60),
            lockout: Duration::from_secs(600),
        });
        let start = Instant::now();
        let s = vec![Subject::Identity("locked@example.test".into())];
        for _ in 0..3 {
            t.record_failure(&s, start);
        }
        // The window has elapsed but the lockout has not: dropping the entry here
        // would silently release them.
        let at = start + Duration::from_secs(61);
        assert_eq!(t.evict_expired(at), 0);
        assert!(t.check(&s, at).is_err());
        // Once the lockout is also past, the entry goes.
        assert_eq!(t.evict_expired(start + Duration::from_secs(601)), 1);
    }

    #[test]
    fn retry_after_rounds_up() {
        // A caller that waits exactly `retry_after_secs` must get in. Rounding
        // down leaves them one millisecond short and refused again, which reads
        // as the lockout being broken.
        let t = AuthThrottle::new(cfg());
        let start = Instant::now();
        let s = subjects();
        for _ in 0..3 {
            t.record_failure(&s, start);
        }
        let at = start + Duration::from_millis(500);
        let locked = t.check(&s, at).expect_err("locked");
        assert!(
            t.check(&s, at + Duration::from_secs(locked.retry_after_secs))
                .is_ok(),
            "waiting retry_after must be enough"
        );
    }
}
