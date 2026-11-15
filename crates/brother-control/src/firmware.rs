use crate::{
    types::{Failure, FailureKind, Model},
    web::selector,
};
use scraper::Html;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, fmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    Main,
    Sub1,
    Sub2,
    Network,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Version {
    parts: Vec<u16>,
    text: String,
}

impl Version {
    pub fn parse(input: &str) -> Result<Self, Failure> {
        let invalid = || {
            Failure::new(
                FailureKind::UnknownResponse,
                "firmware version is outside the supported numeric format",
            )
        };
        if input.len() > 32
            || !input
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            return Err(invalid());
        }
        let parts = input
            .split('.')
            .map(|part| part.parse::<u16>().map_err(|_| invalid()))
            .collect::<Result<Vec<_>, _>>()?;
        if !(2..=4).contains(&parts.len()) {
            return Err(invalid());
        }
        Ok(Self {
            parts,
            text: input.to_owned(),
        })
    }
}
impl TryFrom<String> for Version {
    type Error = Failure;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}
impl From<Version> for String {
    fn from(value: Version) -> Self {
        value.text
    }
}
impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComponentVersion {
    pub component: Component,
    pub version: Version,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Comparison {
    Current,
    UpdateAvailable,
    DeviceNewer,
    Unknown { reason: String },
}

pub fn compare(current: &ComponentVersion, latest: &ComponentVersion) -> Comparison {
    if current.component != latest.component {
        return Comparison::Unknown {
            reason: "firmware components do not match".to_owned(),
        };
    }
    let count = current.version.parts.len().max(latest.version.parts.len());
    let current = current
        .version
        .parts
        .iter()
        .copied()
        .chain(std::iter::repeat(0))
        .take(count);
    let latest = latest
        .version
        .parts
        .iter()
        .copied()
        .chain(std::iter::repeat(0))
        .take(count);
    match current.cmp(latest) {
        Ordering::Less => Comparison::UpdateAvailable,
        Ordering::Equal => Comparison::Current,
        Ordering::Greater => Comparison::DeviceNewer,
    }
}

fn model_name(model: Model) -> &'static str {
    match model {
        Model::HlL2375dw => "HL-L2375DW",
        Model::MfcL2860dw => "MFC-L2860DW",
    }
}
fn product(model: Model) -> &'static str {
    match model {
        Model::HlL2375dw => "hll2375dw",
        Model::MfcL2860dw => "mfcl2860dw",
    }
}
fn invalid_catalog() -> Failure {
    Failure::new(
        FailureKind::UnknownResponse,
        "official firmware catalog did not identify the expected model and components",
    )
}

pub fn latest_components(model: Model, document: &str) -> Result<Vec<ComponentVersion>, Failure> {
    let html = Html::parse_document(document);
    if !html
        .select(&selector("title")?)
        .any(|title| title.text().collect::<String>().contains(model_name(model)))
    {
        return Err(invalid_catalog());
    }
    let history = selector("#pane1 dt")?;
    let latest = html.select(&history).next().ok_or_else(invalid_catalog)?;
    let mut components: Vec<ComponentVersion> = Vec::new();
    for label in latest.select(&selector("nobr")?) {
        let label = label.text().collect::<String>();
        let (component, version) = label.split_once([':', '：']).ok_or_else(invalid_catalog)?;
        let component = match component.trim() {
            "メイン" | "Main" => Component::Main,
            "Sub1" => Component::Sub1,
            "Sub2" => Component::Sub2,
            "Network" => Component::Network,
            _ => return Err(invalid_catalog()),
        };
        if components
            .iter()
            .any(|previous| previous.component == component)
        {
            return Err(invalid_catalog());
        }
        let version = version
            .trim()
            .strip_prefix("ver.")
            .unwrap_or(version.trim());
        components.push(ComponentVersion {
            component,
            version: Version::parse(version)?,
        });
    }
    if components.is_empty() {
        return Err(invalid_catalog());
    }
    Ok(components)
}

pub fn firmware_page(model: Model, index: &str) -> Result<reqwest::Url, Failure> {
    let origin =
        reqwest::Url::parse("https://support.brother.com/").map_err(|_| invalid_catalog())?;
    let html = Html::parse_document(index);
    let mut pages = std::collections::BTreeSet::new();
    for link in html.select(&selector("a[href]")?) {
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        let Ok(url) = origin.join(href) else {
            continue;
        };
        if url.origin() != origin.origin()
            || url.path() != "/g/b/downloadend.aspx"
            || !url.username().is_empty()
            || url.password().is_some()
        {
            continue;
        }
        if url
            .query_pairs()
            .any(|(key, value)| key == "type3" && value == "375")
            && url
                .query_pairs()
                .any(|(key, value)| key == "prod" && value == product(model))
        {
            pages.insert(url.to_string());
        }
    }
    if pages.len() != 1 {
        return Err(invalid_catalog());
    }
    reqwest::Url::parse(pages.first().ok_or_else(invalid_catalog)?).map_err(|_| invalid_catalog())
}

pub fn installed_components(document: &str) -> Result<Vec<ComponentVersion>, Failure> {
    let html = Html::parse_document(document);
    let mut result: Vec<ComponentVersion> = Vec::new();
    for label in html.select(&selector("dt, th")?) {
        let label_text = label
            .text()
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let component = match label_text.trim_end_matches(':') {
            "Main Version" | "Main Firmware Version" => Component::Main,
            "Sub1 Version" | "Sub1 Firmware Version" => Component::Sub1,
            "Sub2 Version" | "Sub2 Firmware Version" => Component::Sub2,
            "Network Firmware Version" => Component::Network,
            _ => continue,
        };
        let value = label
            .next_siblings()
            .filter_map(scraper::ElementRef::wrap)
            .next()
            .ok_or_else(invalid_catalog)?;
        if !matches!(value.value().name(), "dd" | "td") {
            return Err(invalid_catalog());
        }
        let version = Version::parse(value.text().collect::<String>().trim())?;
        if result
            .iter()
            .any(|previous| previous.component == component)
        {
            return Err(invalid_catalog());
        }
        result.push(ComponentVersion { component, version });
    }
    Ok(result)
}

#[derive(Serialize)]
pub struct ComponentComparison {
    pub component: Component,
    pub current: crate::types::Reading<Version>,
    pub latest: crate::types::Reading<Version>,
    pub comparison: Comparison,
}

#[derive(Serialize)]
pub struct FirmwareReport {
    pub printer: crate::types::Printer,
    pub reference_url: String,
    pub current: crate::types::Observation<Vec<ComponentVersion>>,
    pub latest: crate::types::Observation<Vec<ComponentVersion>>,
    pub comparisons: Vec<ComponentComparison>,
}

impl crate::service::Report for FirmwareReport {
    fn succeeded(&self) -> bool {
        matches!(self.latest.reading, crate::types::Reading::Known { .. })
    }
}

fn fetch(client: &reqwest::blocking::Client, url: &str) -> Result<String, Failure> {
    let response = client
        .get(url)
        .send()
        .map_err(crate::ipp_adapter::http_failure)?;
    if !response.status().is_success() {
        return Err(invalid_catalog());
    }
    String::from_utf8(crate::ipp_adapter::bounded_body(response)?).map_err(|_| invalid_catalog())
}

pub fn check(
    store: &crate::config::ConfigStore,
    endpoint: &crate::ipp_adapter::Endpoint,
    timeout: std::time::Duration,
) -> Result<crate::service::Output<FirmwareReport>, Failure> {
    use crate::types::{Observation, Reading, Source};
    let status = crate::ipp_adapter::query(endpoint, timeout)?;
    let mut reference_url = format!(
        "https://support.brother.com/g/b/downloadlist.aspx?c=jp&lang=ja&prod={}&os=10013",
        product(status.printer.model)
    );
    let latest = (|| {
        let client = crate::ipp_adapter::device_client(timeout)?;
        let index = fetch(&client, &reference_url)?;
        let page = firmware_page(status.printer.model, &index)?;
        reference_url = page.to_string();
        latest_components(status.printer.model, &fetch(&client, page.as_str())?)
    })();
    let latest = match latest {
        Ok(value) => Reading::Known { value },
        Err(failure) => Reading::Failed { failure },
    };
    let current = if store
        .path(status.printer.identity)
        .try_exists()
        .map_err(|_| {
            Failure::new(
                FailureKind::Storage,
                "unable to inspect the registered printer",
            )
        })? {
        match store.load(status.printer.identity).and_then(|profile| {
            let lease = store.lease(profile.printer.identity)?;
            crate::device::Device::open(profile, timeout, lease)?.firmware_components()
        }) {
            Ok(value) if !value.is_empty() => Reading::Known { value },
            Ok(_) => Reading::Unknown {
                reason: "printer did not expose recognized firmware component labels".to_owned(),
            },
            Err(failure) => Reading::Failed { failure },
        }
    } else {
        Reading::Missing {
            reason: "component-labelled firmware requires registered Web Based Management access"
                .to_owned(),
        }
    };
    let mut comparisons = Vec::new();
    for component in [
        Component::Main,
        Component::Sub1,
        Component::Sub2,
        Component::Network,
    ] {
        let lookup = |reading: &Reading<Vec<ComponentVersion>>| match reading {
            Reading::Known { value } => value
                .iter()
                .find(|version| version.component == component)
                .cloned(),
            Reading::Missing { .. }
            | Reading::Unknown { .. }
            | Reading::Unsupported { .. }
            | Reading::Failed { .. } => None,
        };
        let observed = lookup(&current);
        let published = lookup(&latest);
        let comparison = match (&observed, &published) {
            (Some(observed), Some(published)) => compare(observed, published),
            (None, _) | (_, None) => Comparison::Unknown { reason: "matching current and official component versions are unavailable; consult the reference URL".to_owned() },
        };
        let reading = |value: Option<ComponentVersion>| match value {
            Some(value) => Reading::Known {
                value: value.version,
            },
            None => Reading::Unknown {
                reason: "component version is unavailable".to_owned(),
            },
        };
        comparisons.push(ComponentComparison {
            component,
            current: reading(observed),
            latest: reading(published),
            comparison,
        });
    }
    let at = crate::ipp_adapter::now()?;
    Ok(crate::service::Output {
        operation: crate::service::OperationName::FirmwareCheck,
        result: FirmwareReport {
            printer: status.printer,
            reference_url,
            current: Observation {
                source: Source::WebManagement,
                observed_at_unix_seconds: at,
                reading: current,
            },
            latest: Observation {
                source: Source::OfficialFirmware,
                observed_at_unix_seconds: at,
                reading: latest,
            },
            comparisons,
        },
    })
}
