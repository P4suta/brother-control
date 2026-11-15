use crate::types::{
    Failure, FailureKind, Host, Identity, Model, Observation, Printer, Reading, Source,
};
use ipp::{operation::IppOperation, parser::IppParser, prelude::*};
use serde::{Deserialize, Serialize};
use std::{
    io::{Cursor, Read},
    num::NonZeroU16,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

pub(crate) fn now() -> Result<u64, Failure> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| Failure::new(FailureKind::Storage, "system clock precedes the Unix epoch"))
}

pub(crate) fn http_failure(error: reqwest::Error) -> Failure {
    if error.is_timeout() {
        Failure::new(FailureKind::Timeout, "device request timed out")
    } else if error.is_connect() {
        Failure::new(FailureKind::Unreachable, "unable to connect to the device")
    } else {
        Failure::new(FailureKind::UnknownResponse, "device HTTP request failed")
    }
}

pub(crate) fn device_client(timeout: Duration) -> Result<reqwest::blocking::Client, Failure> {
    if timeout.is_zero() || timeout > Duration::from_secs(60) {
        return Err(Failure::new(
            FailureKind::InvalidInput,
            "device timeout must be between zero and 60 seconds",
        ));
    }
    reqwest::blocking::Client::builder()
        .no_proxy()
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .connect_timeout(timeout)
        .build()
        .map_err(|_| {
            Failure::new(
                FailureKind::Storage,
                "unable to initialize the device HTTP client",
            )
        })
}

pub(crate) fn bounded_body(response: reqwest::blocking::Response) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    response
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            if matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) {
                Failure::new(FailureKind::Timeout, "device response timed out")
            } else {
                malformed("unable to read the device response")
            }
        })?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(malformed("device response exceeds the size limit"));
    }
    Ok(bytes)
}

pub fn query(endpoint: &Endpoint, timeout: Duration) -> Result<IppStatus, Failure> {
    let response = device_client(timeout)?
        .post(endpoint.http_url())
        .header(reqwest::header::CONTENT_TYPE, "application/ipp")
        .body(request_bytes(endpoint)?)
        .send()
        .map_err(http_failure)?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        return Err(Failure::new(
            FailureKind::Authentication,
            "printer denied the IPP request",
        ));
    }
    if !response.status().is_success() {
        return Err(malformed("printer returned an unsuccessful HTTP status"));
    }
    if !response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/ipp"))
    {
        return Err(malformed("printer response is not application/ipp"));
    }
    parse_response(&bounded_body(response)?, endpoint, now()?)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "EndpointInput", into = "EndpointInput")]
pub struct Endpoint {
    host: Host,
    port: NonZeroU16,
    resource: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointInput {
    host: Host,
    port: u16,
    resource: String,
}

impl Endpoint {
    pub fn new(host: Host, port: u16, resource: &str) -> Result<Self, Failure> {
        let port = NonZeroU16::new(port)
            .ok_or_else(|| Failure::new(FailureKind::InvalidInput, "port must not be zero"))?;
        if !resource.starts_with('/')
            || resource.len() > 512
            || resource.split('/').any(|part| part == "." || part == "..")
            || resource
                .bytes()
                .any(|byte| !byte.is_ascii_alphanumeric() && !b"/-_.".contains(&byte))
        {
            return Err(Failure::new(
                FailureKind::InvalidInput,
                "invalid IPP resource path",
            ));
        }
        Ok(Self {
            host,
            port,
            resource: resource.to_owned(),
        })
    }

    pub fn host(&self) -> &Host {
        &self.host
    }
    pub fn port(&self) -> u16 {
        self.port.get()
    }
    pub fn resource(&self) -> &str {
        &self.resource
    }
    pub fn http_url(&self) -> String {
        format!(
            "http://{}:{}{}",
            self.host.authority(),
            self.port,
            self.resource
        )
    }
}

impl TryFrom<EndpointInput> for Endpoint {
    type Error = Failure;
    fn try_from(input: EndpointInput) -> Result<Self, Self::Error> {
        Self::new(input.host, input.port, &input.resource)
    }
}

impl From<Endpoint> for EndpointInput {
    fn from(endpoint: Endpoint) -> Self {
        Self {
            host: endpoint.host,
            port: endpoint.port.get(),
            resource: endpoint.resource,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrinterState {
    Idle,
    Processing,
    Stopped,
    Sleeping,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IppStatus {
    pub printer: Printer,
    pub state: Observation<PrinterState>,
    pub reasons: Observation<Vec<String>>,
    pub message: Observation<String>,
}

fn malformed(reason: &str) -> Failure {
    Failure::new(FailureKind::UnknownResponse, reason)
}

pub fn request_bytes(endpoint: &Endpoint) -> Result<Vec<u8>, Failure> {
    let uri = format!(
        "ipp://{}:{}{}",
        endpoint.host.authority(),
        endpoint.port,
        endpoint.resource
    )
    .parse()
    .map_err(|_| malformed("invalid printer URI"))?;
    let request = IppOperationBuilder::get_printer_attributes(uri)
        .attributes([
            "printer-uuid",
            "printer-make-and-model",
            "printer-state",
            "printer-state-reasons",
            "printer-state-message",
            "printer-device-id",
        ])
        .build()
        .map_err(|_| malformed("unable to encode attributes request"))?
        .into_ipp_request();
    Ok(request.to_bytes().to_vec())
}

fn attribute<'a>(
    attributes: &'a IppAttributes,
    name: &str,
) -> Result<Option<&'a IppValue>, Failure> {
    let mut found = attributes
        .groups_of(DelimiterTag::PrinterAttributes)
        .flat_map(|group| group.attributes())
        .filter(|attribute| attribute.name().as_str() == name);
    let value = found.next().map(IppAttribute::value);
    if found.next().is_some() {
        return Err(malformed("duplicate printer attribute"));
    }
    Ok(value)
}

fn text(value: &IppValue) -> Result<&str, Failure> {
    match value {
        IppValue::TextWithoutLanguage(value) | IppValue::TextWithLanguage { text: value, .. } => {
            Ok(value.as_ref())
        }
        IppValue::NameWithoutLanguage(value) | IppValue::NameWithLanguage { name: value, .. } => {
            Ok(value.as_str())
        }
        IppValue::Uri(value) | IppValue::UriScheme(value) => Ok(value.as_str()),
        IppValue::Keyword(value) => Ok(value.as_str()),
        IppValue::Integer(_)
        | IppValue::Enum(_)
        | IppValue::OctetString(_)
        | IppValue::Charset(_)
        | IppValue::NaturalLanguage(_)
        | IppValue::RangeOfInteger { .. }
        | IppValue::Boolean(_)
        | IppValue::Array(_)
        | IppValue::Collection(_)
        | IppValue::MimeMediaType(_)
        | IppValue::DateTime(_)
        | IppValue::MemberAttrName(_)
        | IppValue::Resolution { .. }
        | IppValue::NoValue
        | IppValue::Other { .. }
        | IppValue::NonUtf8 { .. } => {
            Err(malformed("printer attribute has an unexpected value type"))
        }
    }
}

fn observation<T>(value: Reading<T>, at: u64) -> Observation<T> {
    Observation {
        source: Source::Ipp,
        observed_at_unix_seconds: at,
        reading: value,
    }
}

pub fn parse_response(bytes: &[u8], endpoint: &Endpoint, at: u64) -> Result<IppStatus, Failure> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(malformed("IPP response exceeds the size limit"));
    }
    let (header, attributes, _) = IppParser::new(Cursor::new(bytes.to_vec()))
        .parse_parts()
        .map_err(|_| malformed("malformed IPP response"))?;
    if header.request_id != 1 {
        return Err(malformed("IPP response request ID does not match"));
    }
    if !header.status_code().is_success() {
        return Err(malformed("printer rejected the IPP attributes request"));
    }
    let identity = Identity::parse(text(
        attribute(&attributes, "printer-uuid")?
            .ok_or_else(|| malformed("printer UUID is missing"))?,
    )?)?;
    let model = Model::parse(text(
        attribute(&attributes, "printer-make-and-model")?
            .ok_or_else(|| malformed("printer model is missing"))?,
    )?)?;
    let reasons = match attribute(&attributes, "printer-state-reasons")? {
        Some(IppValue::Array(values)) => values
            .iter()
            .map(|value| text(value).map(str::to_owned))
            .collect::<Result<Vec<_>, _>>()
            .map(|value| Reading::Known { value }),
        Some(value) => text(value).map(|value| Reading::Known {
            value: Vec::from([value.to_owned()]),
        }),
        None => Ok(Reading::Missing {
            reason: "printer-state-reasons was not returned".to_owned(),
        }),
    };
    let reasons = match reasons {
        Ok(value) => value,
        Err(failure) => Reading::Failed { failure },
    };
    let sleeping = matches!(&reasons, Reading::Known { value } if value.iter().any(|reason| reason == "power-save"));
    let state = match attribute(&attributes, "printer-state")? {
        Some(IppValue::Enum(3)) if sleeping => Reading::Known {
            value: PrinterState::Sleeping,
        },
        Some(IppValue::Enum(3)) => Reading::Known {
            value: PrinterState::Idle,
        },
        Some(IppValue::Enum(4)) => Reading::Known {
            value: PrinterState::Processing,
        },
        Some(IppValue::Enum(5)) => Reading::Known {
            value: PrinterState::Stopped,
        },
        Some(_) => Reading::Unknown {
            reason: "unrecognized printer-state value".to_owned(),
        },
        None => Reading::Missing {
            reason: "printer-state was not returned".to_owned(),
        },
    };
    let message = match attribute(&attributes, "printer-state-message")? {
        Some(value) => match text(value) {
            Ok(value) => Reading::Known {
                value: value.to_owned(),
            },
            Err(failure) => Reading::Failed { failure },
        },
        None => Reading::Missing {
            reason: "printer-state-message was not returned".to_owned(),
        },
    };
    Ok(IppStatus {
        printer: Printer {
            identity,
            model,
            host: endpoint.host.clone(),
        },
        state: observation(state, at),
        reasons: observation(reasons, at),
        message: observation(message, at),
    })
}
