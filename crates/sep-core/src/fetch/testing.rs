use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::client::{Clock, Fetcher, RawResponse, Transport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl Asked {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

pub struct Scripted {
    answers: RefCell<Vec<Result<RawResponse, String>>>,
    asked: Rc<RefCell<Vec<Asked>>>,
}

impl Transport for Scripted {
    fn get(&self, url: &str, headers: &[(&str, String)]) -> Result<RawResponse, String> {
        self.asked.borrow_mut().push(Asked {
            url: url.to_owned(),
            headers: headers
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone()))
                .collect(),
        });
        let mut answers = self.answers.borrow_mut();
        if answers.len() == 1 {
            answers[0].clone()
        } else {
            answers.remove(0)
        }
    }
}

pub struct Recording {
    now: Instant,
    slept: Rc<RefCell<Vec<Duration>>>,
}

impl Clock for Recording {
    fn now(&mut self) -> Instant {
        self.now
    }

    fn sleep(&mut self, duration: Duration) {
        self.slept.borrow_mut().push(duration);
        self.now += duration;
    }
}

pub struct Harness {
    pub fetcher: Fetcher,
    pub asked: Rc<RefCell<Vec<Asked>>>,
    pub slept: Rc<RefCell<Vec<Duration>>>,
}

impl Harness {
    pub fn new(answers: Vec<Result<RawResponse, String>>) -> Self {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let slept = Rc::new(RefCell::new(Vec::new()));
        let transport = Scripted {
            answers: RefCell::new(answers),
            asked: Rc::clone(&asked),
        };
        let clock = Recording {
            now: Instant::now(),
            slept: Rc::clone(&slept),
        };
        Harness {
            fetcher: Fetcher::new(Box::new(transport)).with_clock(Box::new(clock)),
            asked,
            slept,
        }
    }

    pub fn urls(&self) -> Vec<String> {
        self.asked.borrow().iter().map(|a| a.url.clone()).collect()
    }
}

pub fn ok(url: &str, body: &str) -> RawResponse {
    RawResponse {
        status: 200,
        status_text: "OK".into(),
        url: url.into(),
        content_type: "text/html".into(),
        body: body.as_bytes().to_vec(),
        ..Default::default()
    }
}

pub fn status(code: u16, text: &str) -> RawResponse {
    RawResponse {
        status: code,
        status_text: text.into(),
        ..Default::default()
    }
}
