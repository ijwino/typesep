use std::time::Duration;

use super::client::{RawResponse, Transport};

pub struct UreqTransport {
    agent: ureq::Agent,
    timeout: Duration,
}

impl UreqTransport {
    pub fn new(timeout: Duration) -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .build();
        Self {
            agent: config.into(),
            timeout,
        }
    }
}

fn header(response: &ureq::http::Response<ureq::Body>, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn describe(err: &ureq::Error, timeout: Duration) -> String {
    match err {
        ureq::Error::Timeout(_) => format!("timed out after {}s", timeout.as_secs_f64()),
        ureq::Error::HostNotFound => "host not found".to_owned(),
        ureq::Error::ConnectionFailed => "connection failed".to_owned(),
        ureq::Error::TooManyRedirects => "too many redirects".to_owned(),
        ureq::Error::Io(io) => io.to_string(),
        other => other.to_string(),
    }
}

impl Transport for UreqTransport {
    fn get(&self, url: &str, headers: &[(&str, String)]) -> Result<RawResponse, String> {
        let mut request = self.agent.get(url);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let mut response = request.call().map_err(|err| describe(&err, self.timeout))?;

        let status = response.status().as_u16();
        let status_text = response
            .status()
            .canonical_reason()
            .unwrap_or_default()
            .to_owned();
        let etag = header(&response, "etag");
        let last_modified = header(&response, "last-modified");
        let retry_after = header(&response, "retry-after");
        let content_type = header(&response, "content-type").unwrap_or_default();
        let final_url = {
            use ureq::ResponseExt;
            response.get_uri().to_string()
        };
        let body = if (200..300).contains(&status) {
            response
                .body_mut()
                .with_config()
                .limit(MAX_BODY)
                .read_to_vec()
                .map_err(|err| describe(&err, self.timeout))?
        } else {
            Vec::new()
        };

        Ok(RawResponse {
            status,
            status_text,
            url: final_url,
            etag,
            last_modified,
            retry_after,
            content_type,
            body,
        })
    }
}

const MAX_BODY: u64 = 64 * 1024 * 1024;
