pub mod dom;
pub mod js;

pub use regex;
pub use scraper;

#[macro_export]
macro_rules! lazy_regex {
    ($name:ident = $pattern:expr) => {
        static $name: std::sync::LazyLock<$crate::regex::Regex> = std::sync::LazyLock::new(|| {
            let pattern = $pattern;
            $crate::regex::Regex::new(pattern.as_ref()).expect(concat!("regex ", stringify!($name)))
        });
    };
}

#[macro_export]
macro_rules! lazy_selector {
    ($name:ident = $text:literal) => {
        static $name: std::sync::LazyLock<$crate::scraper::Selector> =
            std::sync::LazyLock::new(|| $crate::scraper::Selector::parse($text).expect($text));
    };
}
