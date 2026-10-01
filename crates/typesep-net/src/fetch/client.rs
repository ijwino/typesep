use std::time::{Duration, Instant};

pub const DEFAULT_DELAY_MS: u64 = 5_000;
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const DEFAULT_RETRIES: u32 = 2;
const MAX_BACKOFF_MS: u64 = 60_000;
const WORTH_SAYING_MS: u64 = 2_000;

pub fn user_agent(version: &str, homepage: &str) -> String {
    let mut ua = format!("typesep/{version} (personal PDF generation from public SEP pages");
    if !homepage.is_empty() {
        ua.push_str(&format!("; +{homepage}"));
    }
    ua.push(')');
    ua
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    #[error("could not fetch {url}: {reason}")]
    Transport { url: String, reason: String },
    #[error("HTTP {status} {status_text} for {url}")]
    Status {
        url: String,
        status: u16,
        status_text: String,
    },
}

impl FetchError {
    pub fn status(&self) -> Option<u16> {
        match self {
            FetchError::Status { status, .. } => Some(*status),
            FetchError::Transport { .. } => None,
        }
    }

    pub fn is_gone(&self) -> bool {
        matches!(self.status(), Some(404 | 410))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FetchOptions<'a> {
    pub etag: Option<&'a str>,
    pub last_modified: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchResult {
    NotModified,
    Fetched(Fetched),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub status: u16,
    pub url: String,
    pub body: Vec<u8>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawResponse {
    pub status: u16,
    pub status_text: String,
    pub url: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub retry_after: Option<String>,
    pub content_type: String,
    pub body: Vec<u8>,
}

pub trait Transport {
    fn get(&self, url: &str, headers: &[(&str, String)]) -> Result<RawResponse, String>;
}

pub trait Clock {
    fn now(&mut self) -> Instant;
    fn sleep(&mut self, duration: Duration);
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&mut self) -> Instant {
        Instant::now()
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loudness {
    Detail,
    Notice,
}

type Log = Box<dyn FnMut(Loudness, &str)>;

pub struct Fetcher {
    delay: Duration,
    retries: u32,
    user_agent: String,
    transport: Box<dyn Transport>,
    clock: Box<dyn Clock>,
    log: Log,
    last_request_at: Option<Instant>,
    requests: u64,
}

impl Fetcher {
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            delay: Duration::from_millis(DEFAULT_DELAY_MS),
            retries: DEFAULT_RETRIES,
            user_agent: user_agent(env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_REPOSITORY")),
            transport,
            clock: Box::new(SystemClock),
            log: Box::new(|_, _| {}),
            last_request_at: None,
            requests: 0,
        }
    }

    pub fn live(delay: Duration, timeout: Duration) -> Self {
        let mut fetcher = Self::new(Box::new(super::http::UreqTransport::new(timeout)));
        fetcher.delay = delay;
        fetcher
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn with_retries(mut self, retries: u32) -> Self {
        self.retries = retries;
        self
    }

    pub fn with_clock(mut self, clock: Box<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub fn with_log(mut self, log: impl FnMut(Loudness, &str) + 'static) -> Self {
        self.log = Box::new(log);
        self
    }

    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    pub fn requests(&self) -> u64 {
        self.requests
    }

    fn say(&mut self, loudness: Loudness, message: &str) {
        (self.log)(loudness, message);
    }

    fn wait_turn(&mut self) {
        let Some(last) = self.last_request_at else {
            return;
        };
        let elapsed = self.clock.now().saturating_duration_since(last);
        let Some(wait) = self.delay.checked_sub(elapsed) else {
            return;
        };
        if wait.is_zero() {
            return;
        }
        if wait >= Duration::from_millis(WORTH_SAYING_MS) {
            let seconds = wait.as_millis().div_ceil(1000);
            self.say(
                Loudness::Notice,
                &format!("waiting {seconds}s before the next request"),
            );
        }
        self.clock.sleep(wait);
    }

    fn backoff(&self, attempt: u32) -> Duration {
        let base = self.delay.as_millis().max(u128::from(WORTH_SAYING_MS));
        let grown = base.saturating_mul(1u128 << attempt.min(32));
        Duration::from_millis(grown.min(u128::from(MAX_BACKOFF_MS)) as u64)
    }

    pub fn get(&mut self, url: &str, opts: FetchOptions<'_>) -> Result<FetchResult, FetchError> {
        self.wait_turn();
        let outcome = self.attempts(url, opts);
        self.last_request_at = Some(self.clock.now());
        outcome
    }

    fn attempts(&mut self, url: &str, opts: FetchOptions<'_>) -> Result<FetchResult, FetchError> {
        let mut headers: Vec<(&str, String)> = vec![("user-agent", self.user_agent.clone())];
        if let Some(etag) = opts.etag {
            headers.push(("if-none-match", etag.to_owned()));
        }
        if let Some(last_modified) = opts.last_modified {
            headers.push(("if-modified-since", last_modified.to_owned()));
        }

        for attempt in 0.. {
            let retry = if attempt > 0 {
                format!(" (retry {attempt})")
            } else {
                String::new()
            };
            self.say(Loudness::Detail, &format!("GET {url}{retry}"));
            self.requests += 1;
            let answer = self.transport.get(url, &headers);

            let res = match answer {
                Ok(res) => res,
                Err(reason) => {
                    if attempt >= self.retries {
                        return Err(FetchError::Transport {
                            url: url.to_owned(),
                            reason,
                        });
                    }
                    let wait = self.backoff(attempt);
                    self.say(
                        Loudness::Notice,
                        &format!("{reason}; retrying in {}s", round_secs(wait)),
                    );
                    self.clock.sleep(wait);
                    continue;
                }
            };

            if res.status == 304 {
                return Ok(FetchResult::NotModified);
            }
            if (200..300).contains(&res.status) {
                return Ok(FetchResult::Fetched(Fetched {
                    status: res.status,
                    url: res.url,
                    body: res.body,
                    etag: res.etag,
                    last_modified: res.last_modified,
                    content_type: res.content_type,
                }));
            }

            let transient = res.status == 429 || res.status >= 500;
            if !transient || attempt >= self.retries {
                return Err(FetchError::Status {
                    url: url.to_owned(),
                    status: res.status,
                    status_text: res.status_text,
                });
            }
            let wait = res
                .retry_after
                .as_deref()
                .and_then(retry_after)
                .map(|wait| wait.min(Duration::from_millis(MAX_BACKOFF_MS)))
                .unwrap_or_else(|| self.backoff(attempt));
            self.say(
                Loudness::Notice,
                &format!("HTTP {}; retrying in {}s", res.status, round_secs(wait)),
            );
            self.clock.sleep(wait);
        }
        unreachable!("the attempt loop returns")
    }
}

fn round_secs(wait: Duration) -> u128 {
    (wait.as_millis() + 500) / 1000
}

fn retry_after(header: &str) -> Option<Duration> {
    if !header.is_empty() && header.bytes().all(|b| b.is_ascii_digit()) {
        return header.parse::<u64>().ok().map(Duration::from_secs);
    }
    let at = http_date(header)?;
    let left = at - time::OffsetDateTime::now_utc();
    Some(if left.is_negative() {
        Duration::ZERO
    } else {
        left.unsigned_abs()
    })
}

fn http_date(text: &str) -> Option<time::OffsetDateTime> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let rest = text.split_once(", ").map(|(_, rest)| rest).unwrap_or(text);
    let mut parts = rest.split(' ');
    let day: u8 = parts.next()?.parse().ok()?;
    let month_name = parts.next()?;
    let year: i32 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':');
    let hour: u8 = clock.next()?.parse().ok()?;
    let minute: u8 = clock.next()?.parse().ok()?;
    let second: u8 = clock.next()?.parse().ok()?;
    let month = MONTHS.iter().position(|m| *m == month_name)? as u8 + 1;
    let date =
        time::Date::from_calendar_date(year, time::Month::try_from(month).ok()?, day).ok()?;
    let at = date.with_hms(hour, minute, second).ok()?;
    Some(at.assume_utc())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::testing::{Harness, ok as ok_at, status};

    const URL: &str = "https://plato.stanford.edu/entries/x/";

    fn ok(body: &str) -> RawResponse {
        ok_at(URL, body)
    }

    fn harness(answers: Vec<Result<RawResponse, String>>) -> Harness {
        Harness::new(answers)
    }

    #[test]
    fn requests_are_one_at_a_time_and_a_delay_apart() {
        let mut h = harness(vec![Ok(ok("one"))]);
        for _ in 0..3 {
            h.fetcher.get(URL, FetchOptions::default()).unwrap();
        }
        assert_eq!(h.asked.borrow().len(), 3);
        assert_eq!(
            *h.slept.borrow(),
            vec![
                Duration::from_millis(DEFAULT_DELAY_MS),
                Duration::from_millis(DEFAULT_DELAY_MS)
            ],
            "the first request goes out at once; each later one waits out the gap"
        );
    }

    #[test]
    fn every_request_identifies_the_tool() {
        let mut h = harness(vec![Ok(ok("one"))]);
        h.fetcher.get(URL, FetchOptions::default()).unwrap();
        let asked = h.asked.borrow();
        let ua = asked[0]
            .header("user-agent")
            .expect("a User-Agent")
            .to_owned();
        assert!(ua.starts_with("typesep/"), "{ua}");
        assert!(ua.contains("personal PDF generation from public SEP pages"));
        assert!(ua.contains("+https://github.com/"));
    }

    #[test]
    fn a_conditional_request_offers_what_the_library_holds() {
        let mut h = harness(vec![Ok(status(304, "Not Modified"))]);
        let result = h
            .fetcher
            .get(
                URL,
                FetchOptions {
                    etag: Some("\"abc\""),
                    last_modified: Some("Mon, 24 Sep 2018 22:05:37 GMT"),
                },
            )
            .unwrap();
        assert_eq!(result, FetchResult::NotModified);
        let asked = h.asked.borrow();
        assert_eq!(asked[0].header("if-none-match"), Some("\"abc\""));
        assert_eq!(
            asked[0].header("if-modified-since"),
            Some("Mon, 24 Sep 2018 22:05:37 GMT")
        );
    }

    #[test]
    fn a_404_is_an_error_that_carries_its_status() {
        let mut h = harness(vec![Ok(status(404, "Not Found"))]);
        let err = h.fetcher.get(URL, FetchOptions::default()).unwrap_err();
        assert_eq!(err.status(), Some(404));
        assert!(err.is_gone());
        assert_eq!(err.to_string(), format!("HTTP 404 Not Found for {URL}"));
        assert_eq!(h.asked.borrow().len(), 1, "a 404 is not retried");
    }

    #[test]
    fn a_server_that_is_out_of_breath_is_given_time() {
        let mut h = harness(vec![
            Ok(RawResponse {
                retry_after: Some("3".into()),
                ..status(429, "Too Many Requests")
            }),
            Ok(ok("at last")),
        ]);
        let result = h.fetcher.get(URL, FetchOptions::default()).unwrap();
        assert!(matches!(result, FetchResult::Fetched(_)));
        assert_eq!(
            *h.slept.borrow(),
            vec![Duration::from_secs(3)],
            "Retry-After is honoured over the backoff"
        );
    }

    #[test]
    fn a_long_retry_after_waits_no_more_than_a_minute() {
        let mut h = harness(vec![
            Ok(RawResponse {
                retry_after: Some("86400".into()),
                ..status(503, "Service Unavailable")
            }),
            Ok(ok("at last")),
        ]);
        h.fetcher.get(URL, FetchOptions::default()).unwrap();
        assert_eq!(
            *h.slept.borrow(),
            vec![Duration::from_millis(MAX_BACKOFF_MS)]
        );
    }

    #[test]
    fn a_transient_failure_is_retried_and_then_given_up_on() {
        let mut h = harness(vec![
            Ok(status(503, "Service Unavailable")),
            Ok(status(503, "Service Unavailable")),
            Ok(status(503, "Service Unavailable")),
        ]);
        let err = h.fetcher.get(URL, FetchOptions::default()).unwrap_err();
        assert_eq!(err.status(), Some(503));
        assert_eq!(h.asked.borrow().len(), 3, "the first go and two retries");
        assert_eq!(
            *h.slept.borrow(),
            vec![
                Duration::from_millis(DEFAULT_DELAY_MS),
                Duration::from_millis(2 * DEFAULT_DELAY_MS)
            ],
            "the backoff doubles"
        );
    }

    #[test]
    fn a_broken_connection_is_retried_and_described() {
        let mut h = harness(vec![
            Err("connection refused".to_owned()),
            Ok(ok("second time lucky")),
        ]);
        let result = h.fetcher.get(URL, FetchOptions::default()).unwrap();
        assert!(matches!(result, FetchResult::Fetched(_)));

        let mut h = harness(vec![Err("timed out after 30s".to_owned())]);
        let err = h.fetcher.get(URL, FetchOptions::default()).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("could not fetch {URL}: timed out after 30s")
        );
        assert_eq!(err.status(), None);
    }

    #[test]
    fn the_backoff_grows_to_a_minute_and_stops() {
        let fetcher = harness(vec![Ok(ok(""))]).fetcher.with_delay(Duration::ZERO);
        assert_eq!(fetcher.backoff(0), Duration::from_secs(2));
        assert_eq!(fetcher.backoff(3), Duration::from_secs(16));
        assert_eq!(fetcher.backoff(10), Duration::from_secs(60));
    }

    #[test]
    fn retry_after_reads_seconds_and_dates() {
        assert_eq!(retry_after("7"), Some(Duration::from_secs(7)));
        assert_eq!(retry_after("soon"), None);
        assert_eq!(
            retry_after("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(Duration::ZERO),
            "a date already past means no wait at all"
        );
        let soon = time::OffsetDateTime::now_utc() + time::Duration::seconds(30);
        let header = format!(
            "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
            soon.weekday().to_string()[..3].to_owned(),
            soon.day(),
            &soon.month().to_string()[..3],
            soon.year(),
            soon.hour(),
            soon.minute(),
            soon.second()
        );
        let wait = retry_after(&header).expect("an HTTP date");
        assert!(
            wait <= Duration::from_secs(30) && wait >= Duration::from_secs(28),
            "{wait:?}"
        );
    }

    #[test]
    fn the_user_agent_names_the_tool_and_where_to_complain() {
        assert_eq!(
            user_agent("0.1.0", "https://example.com/x"),
            "typesep/0.1.0 (personal PDF generation from public SEP pages; +https://example.com/x)"
        );
        assert_eq!(
            user_agent("0.1.0", ""),
            "typesep/0.1.0 (personal PDF generation from public SEP pages)"
        );
    }
}
