// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the server knows about a client that the event does not say: where it is, and what
//! browser it runs on. Both come from the connection (its address and `User-Agent`), so they are
//! worked out once per request and stamped on every event of it.

use std::{net::IpAddr, sync::Arc};

use moka::sync::Cache;
use serde_json::{Map, Value};
use woothee::parser::Parser;

use crate::{
    domain::rum::RumApplicationSettings,
    infra::enrichment::{GeoLocation, GeoLookup},
};

const AGENT_CACHE_ENTRIES: u64 = 1024;
const MAX_AGENT_LEN: usize = 512;

/// What a `User-Agent` says, read the way an event would say it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentInfo {
    pub browser_name: Option<String>,
    pub browser_version: Option<String>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub device_type: Option<&'static str>,
}

/// Looks clients up. One lives for the whole process: the agent cache is shared by every request.
pub struct Enricher {
    geo: Arc<dyn GeoLookup>,
    agents: Cache<String, Arc<AgentInfo>>,
}

impl Enricher {
    pub fn new(geo: Arc<dyn GeoLookup>) -> Self {
        Self {
            geo,
            agents: Cache::new(AGENT_CACHE_ENTRIES),
        }
    }

    /// The client of one request. The lookups are made here, once, however many events follow.
    pub fn origin(&self, ip: Option<IpAddr>, user_agent: Option<&str>) -> Origin {
        Origin {
            ip,
            place: ip.and_then(|ip| self.geo.lookup(ip)),
            agent: user_agent.map(|agent| self.agent(agent)),
        }
    }

    fn agent(&self, user_agent: &str) -> Arc<AgentInfo> {
        let user_agent = &user_agent[..floor_boundary(user_agent, MAX_AGENT_LEN)];

        self.agents
            .get_with(user_agent.to_string(), || Arc::new(parse_agent(user_agent)))
    }
}

fn floor_boundary(text: &str, limit: usize) -> usize {
    let mut end = limit.min(text.len());

    while !text.is_char_boundary(end) {
        end -= 1;
    }

    end
}

/// The client of a request: its address, the place the address is in, and its browser.
#[derive(Debug, Clone, Default)]
pub struct Origin {
    pub ip: Option<IpAddr>,
    pub place: Option<GeoLocation>,
    pub agent: Option<Arc<AgentInfo>>,
}

impl Origin {
    /// Writes what the connection says into a row. The application decides what is kept; the
    /// browser, system and device are filled in only where the event left them out and only for
    /// events from a browser, whose `User-Agent` describes the device the event came from.
    pub fn stamp(&self, row: &mut Map<String, Value>, settings: &RumApplicationSettings) {
        if settings.collect_ip
            && let Some(ip) = self.ip
        {
            row.insert("network.client.ip".into(), Value::String(ip.to_string()));
        }

        if settings.collect_geo
            && let Some(place) = &self.place
        {
            stamp_place(row, place);
        }

        let from_browser = row.get("source").and_then(Value::as_str) == Some("browser");

        if from_browser && let Some(agent) = &self.agent {
            stamp_agent(row, agent);
        }
    }
}

fn stamp_place(row: &mut Map<String, Value>, place: &GeoLocation) {
    let texts = [
        ("geo.country_iso_code", &place.country),
        ("geo.country", &place.country_name),
        ("geo.continent_code", &place.continent_code),
        ("geo.continent", &place.continent),
        ("geo.country_subdivision", &place.region),
        ("geo.city", &place.city),
    ];

    for (name, value) in texts {
        if let Some(value) = value {
            row.insert(name.into(), Value::String(value.clone()));
        }
    }

    for (name, value) in [
        ("geo.latitude", place.latitude),
        ("geo.longitude", place.longitude),
    ] {
        if let Some(number) = value.and_then(serde_json::Number::from_f64) {
            row.insert(name.into(), Value::Number(number));
        }
    }
}

fn stamp_agent(row: &mut Map<String, Value>, agent: &AgentInfo) {
    let mut set = |name: &str, value: Option<&str>| {
        if let Some(value) = value
            && !row.contains_key(name)
        {
            row.insert(name.into(), Value::String(value.to_string()));
        }
    };

    set("browser.name", agent.browser_name.as_deref());
    set("browser.version", agent.browser_version.as_deref());
    set(
        "browser.version_major",
        major(agent.browser_version.as_deref()),
    );
    set("os.name", agent.os_name.as_deref());
    set("os.version", agent.os_version.as_deref());
    set("os.version_major", major(agent.os_version.as_deref()));
    set("device.type", agent.device_type);
}

fn major(version: Option<&str>) -> Option<&str> {
    version
        .and_then(|version| version.split('.').next())
        .filter(|major| !major.is_empty())
}

fn parse_agent(user_agent: &str) -> AgentInfo {
    let Some(result) = Parser::new().parse(user_agent) else {
        return AgentInfo::default();
    };

    let known = |value: &str| (value != "UNKNOWN" && !value.is_empty()).then(|| value.to_string());

    let (os_name, os_version) = system(result.os, &result.os_version);

    AgentInfo {
        browser_name: known(result.name),
        browser_version: known(result.version),
        os_name,
        os_version,
        device_type: device_type(result.category, result.os, user_agent),
    }
}

/// The operating system as `os.name` and `os.version` say it, from what woothee reports: its `os`
/// ("Windows 10", "Mac OSX", "iPhone") and, for some, a separate `os_version`.
fn system(os: &str, os_version: &str) -> (Option<String>, Option<String>) {
    let version =
        |value: &str| (value != "UNKNOWN" && !value.is_empty()).then(|| value.to_string());

    if os == "UNKNOWN" || os.is_empty() {
        return (None, None);
    }

    if let Some(rest) = os.strip_prefix("Windows") {
        let rest = rest.trim();

        return (
            Some("Windows".into()),
            version(rest).or_else(|| version(os_version)),
        );
    }

    let name = match os {
        "Mac OSX" | "Mac OS X" => "macOS",
        "iPhone" | "iPad" | "iPod" | "iOS" => "iOS",
        "ChromeOS" => "Chrome OS",
        other => other,
    };

    (Some(name.to_string()), version(os_version))
}

fn device_type(category: &str, os: &str, user_agent: &str) -> Option<&'static str> {
    match category {
        "crawler" => Some("bot"),
        "pc" => Some("desktop"),
        "smartphone" | "mobilephone" => {
            let tablet = os == "iPad"
                || user_agent.contains("iPad")
                || (os == "Android" && !user_agent.contains("Mobile"));

            Some(if tablet { "tablet" } else { "mobile" })
        }
        "appliance" | "misc" => Some("other"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::*;

    const CHROME_MAC: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
    const SAFARI_IPHONE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";
    const FIREFOX_WINDOWS: &str =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0";
    const GOOGLEBOT: &str =
        "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)";

    #[derive(Default)]
    struct FakeGeo {
        lookups: AtomicUsize,
    }

    impl GeoLookup for FakeGeo {
        fn lookup(&self, ip: IpAddr) -> Option<GeoLocation> {
            self.lookups.fetch_add(1, Ordering::Relaxed);

            (ip.to_string() == "203.0.113.7").then(|| GeoLocation {
                country: Some("DE".into()),
                country_name: Some("Germany".into()),
                continent_code: Some("EU".into()),
                continent: Some("Europe".into()),
                region: Some("Berlin".into()),
                city: Some("Berlin".into()),
                latitude: Some(52.52),
                longitude: Some(13.4),
            })
        }
    }

    fn ip(value: &str) -> Option<IpAddr> {
        Some(value.parse().unwrap())
    }

    fn row(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn a_browser_is_read_from_its_user_agent() {
        let agent = parse_agent(CHROME_MAC);

        assert_eq!(agent.browser_name.as_deref(), Some("Chrome"));
        assert_eq!(agent.browser_version.as_deref(), Some("130.0.0.0"));
        assert_eq!(agent.os_name.as_deref(), Some("macOS"));
        assert_eq!(agent.device_type, Some("desktop"));

        let phone = parse_agent(SAFARI_IPHONE);

        assert_eq!(phone.os_name.as_deref(), Some("iOS"));
        assert_eq!(phone.device_type, Some("mobile"));

        let windows = parse_agent(FIREFOX_WINDOWS);

        assert_eq!(windows.os_name.as_deref(), Some("Windows"));
        assert_eq!(windows.browser_name.as_deref(), Some("Firefox"));
    }

    #[test]
    fn a_crawler_is_a_bot() {
        assert_eq!(parse_agent(GOOGLEBOT).device_type, Some("bot"));
    }

    #[test]
    fn an_unreadable_agent_says_nothing() {
        let agent = parse_agent("");

        assert_eq!(agent.browser_name, None);
        assert_eq!(agent.device_type, None);
    }

    #[test]
    fn the_connection_is_looked_up_once_per_request() {
        let geo = Arc::new(FakeGeo::default());
        let enricher = Enricher::new(geo.clone());
        let origin = enricher.origin(ip("203.0.113.7"), Some(CHROME_MAC));
        let settings = RumApplicationSettings::default();

        for _ in 0..3 {
            let mut event = row(json!({"source": "browser"}));

            origin.stamp(&mut event, &settings);
        }

        assert_eq!(geo.lookups.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn the_place_and_the_address_are_stamped() {
        let enricher = Enricher::new(Arc::new(FakeGeo::default()));
        let origin = enricher.origin(ip("203.0.113.7"), None);
        let mut event = row(json!({"source": "flutter"}));

        origin.stamp(&mut event, &RumApplicationSettings::default());

        assert_eq!(event["network.client.ip"], "203.0.113.7");
        assert_eq!(event["geo.country_iso_code"], "DE");
        assert_eq!(event["geo.country"], "Germany");
        assert_eq!(event["geo.continent_code"], "EU");
        assert_eq!(event["geo.country_subdivision"], "Berlin");
        assert_eq!(event["geo.latitude"], 52.52);
    }

    #[test]
    fn the_application_decides_what_is_kept() {
        let enricher = Enricher::new(Arc::new(FakeGeo::default()));
        let origin = enricher.origin(ip("203.0.113.7"), None);

        let mut no_ip = row(json!({"source": "browser"}));
        origin.stamp(
            &mut no_ip,
            &RumApplicationSettings {
                collect_ip: false,
                ..RumApplicationSettings::default()
            },
        );
        assert!(!no_ip.contains_key("network.client.ip"));
        assert_eq!(no_ip["geo.country_iso_code"], "DE");

        let mut no_geo = row(json!({"source": "browser"}));
        origin.stamp(
            &mut no_geo,
            &RumApplicationSettings {
                collect_geo: false,
                ..RumApplicationSettings::default()
            },
        );
        assert_eq!(no_geo["network.client.ip"], "203.0.113.7");
        assert!(!no_geo.keys().any(|key| key.starts_with("geo.")));
    }

    #[test]
    fn an_unknown_address_adds_no_place() {
        let enricher = Enricher::new(Arc::new(FakeGeo::default()));
        let origin = enricher.origin(ip("10.1.2.3"), None);
        let mut event = row(json!({"source": "browser"}));

        origin.stamp(&mut event, &RumApplicationSettings::default());

        assert_eq!(event["network.client.ip"], "10.1.2.3");
        assert!(!event.keys().any(|key| key.starts_with("geo.")));
    }

    #[test]
    fn a_browser_event_gets_what_it_left_out_and_keeps_what_it_said() {
        let enricher = Enricher::new(Arc::new(FakeGeo::default()));
        let origin = enricher.origin(None, Some(CHROME_MAC));
        let settings = RumApplicationSettings::default();

        let mut event = row(json!({"source": "browser", "os.name": "Custom OS"}));

        origin.stamp(&mut event, &settings);

        assert_eq!(event["browser.name"], "Chrome");
        assert_eq!(event["browser.version_major"], "130");
        assert_eq!(event["os.name"], "Custom OS");
        assert_eq!(event["device.type"], "desktop");
    }

    #[test]
    fn a_mobile_sdk_is_not_described_by_the_user_agent_of_its_http_client() {
        let enricher = Enricher::new(Arc::new(FakeGeo::default()));
        let origin = enricher.origin(None, Some("Dart/3.9 (dart:io)"));
        let mut event = row(json!({"source": "flutter"}));

        origin.stamp(&mut event, &RumApplicationSettings::default());

        assert!(!event.contains_key("browser.name"));
        assert!(!event.contains_key("device.type"));
    }

    #[test]
    fn the_agent_cache_returns_the_same_parse() {
        let enricher = Enricher::new(Arc::new(FakeGeo::default()));
        let first = enricher.agent(CHROME_MAC);
        let second = enricher.agent(CHROME_MAC);

        assert!(Arc::ptr_eq(&first, &second));
    }
}
