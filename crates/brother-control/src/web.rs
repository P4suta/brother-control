use crate::{
    config::{Fingerprint, Profile, Secret},
    ipp_adapter::{bounded_body, http_failure},
    types::{Failure, FailureKind, Host, Model},
};
use reqwest::blocking::{Client, ClientBuilder, Response};
use scraper::{Html, Selector};
use serde::Serialize;
use std::{net::ToSocketAddrs, time::Duration};
use x509_parser::{extensions::GeneralName, prelude::*};

fn unknown(reason: &str) -> Failure {
    Failure::new(FailureKind::UnknownResponse, reason)
}

pub(crate) fn selector(input: &str) -> Result<Selector, Failure> {
    Selector::parse(input).map_err(|_| unknown("invalid management form selector"))
}

#[derive(Clone, Debug, Serialize)]
pub struct CertificateInfo {
    pub sha256: Fingerprint,
    pub tls_host: Option<Host>,
    #[serde(skip)]
    pub certificate_der: Vec<u8>,
}

pub fn certificate_info(bytes: &[u8]) -> Result<CertificateInfo, Failure> {
    if bytes.is_empty() || bytes.len() > 65_536 {
        return Err(Failure::new(
            FailureKind::Certificate,
            "certificate exceeds the size limit",
        ));
    }
    let (remaining, certificate) = X509Certificate::from_der(bytes)
        .map_err(|_| Failure::new(FailureKind::Certificate, "invalid device certificate"))?;
    if !remaining.is_empty() {
        return Err(Failure::new(
            FailureKind::Certificate,
            "unexpected data after the device certificate",
        ));
    }
    if !certificate.validity().is_valid() {
        return Err(Failure::new(
            FailureKind::Certificate,
            "device certificate is outside its validity period",
        ));
    }
    let names = certificate.subject_alternative_name().map_err(|_| {
        Failure::new(
            FailureKind::Certificate,
            "invalid certificate subject names",
        )
    })?;
    let name = names.and_then(|names| {
        names.value.general_names.iter().find_map(|name| {
            if let GeneralName::DNSName(value) = name {
                Some(*value)
            } else {
                None
            }
        })
    });
    Ok(CertificateInfo {
        sha256: Fingerprint::of(bytes),
        tls_host: name.map(Host::parse).transpose()?,
        certificate_der: bytes.to_vec(),
    })
}

fn builder(timeout: Duration) -> Result<ClientBuilder, Failure> {
    if timeout.is_zero() || timeout > Duration::from_secs(60) {
        return Err(Failure::new(
            FailureKind::InvalidInput,
            "web timeout must be greater than zero and at most 60 seconds",
        ));
    }
    Ok(Client::builder()
        .tls_backend_rustls()
        .no_proxy()
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .timeout(timeout)
        .connect_timeout(timeout)
        .tls_info(true))
}

pub fn inspect_certificate(
    host: &Host,
    port: u16,
    timeout: Duration,
) -> Result<CertificateInfo, Failure> {
    if port == 0 {
        return Err(Failure::new(
            FailureKind::InvalidInput,
            "web port must not be zero",
        ));
    }
    let response = builder(timeout)?
        .tls_danger_accept_invalid_certs(true)
        .build()
        .map_err(|_| unknown("unable to initialize certificate inspection"))?
        .get(format!(
            "https://{}:{port}/home/status.html",
            host.authority()
        ))
        .send()
        .map_err(http_failure)?;
    let certificate = response
        .extensions()
        .get::<reqwest::tls::TlsInfo>()
        .and_then(|info| info.peer_certificate())
        .ok_or_else(|| {
            Failure::new(
                FailureKind::Certificate,
                "server did not provide its TLS certificate",
            )
        })?;
    certificate_info(certificate)
}

pub struct LoginForm {
    password: String,
    hidden: Vec<(String, String)>,
}

impl LoginForm {
    pub fn password_field(&self) -> &str {
        &self.password
    }
}

pub fn page_model(document: &str) -> Result<Model, Failure> {
    let html = Html::parse_document(document);
    let title = selector("title")?;
    let title = html
        .select(&title)
        .next()
        .ok_or_else(|| unknown("management page has no model title"))?;
    Model::parse(&title.text().collect::<String>())
}

pub const fn status_path(model: Model) -> &'static str {
    match model {
        Model::HlL2375dw => "/general/status.html",
        Model::MfcL2860dw => "/home/status.html",
    }
}

pub fn login_form(document: &str) -> Result<LoginForm, Failure> {
    let model = page_model(document)?;
    let html = Html::parse_document(document);
    let password_selector = selector("input[type=password]")?;
    let hidden_selector = selector("input[type=hidden]")?;
    let forms = selector("form")?;
    let mut result = None;
    for form in html.select(&forms) {
        let passwords: Vec<_> = form.select(&password_selector).collect();
        if passwords.is_empty() {
            continue;
        }
        if passwords.len() != 1
            || result.is_some()
            || form.value().attr("action") != Some(status_path(model))
            || !form
                .value()
                .attr("method")
                .is_some_and(|method| method.eq_ignore_ascii_case("post"))
        {
            return Err(unknown("unrecognized management login form"));
        }
        let input = passwords[0];
        let name = input
            .value()
            .attr("name")
            .ok_or_else(|| unknown("password field has no name"))?;
        if input.value().attr("id") != Some("LogBox")
            || name.is_empty()
            || name.len() > 32
            || !name.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(unknown("unrecognized management password field"));
        }
        let password_name = name;
        let mut hidden: Vec<(String, String)> = Vec::new();
        for input in form.select(&hidden_selector) {
            let name = input
                .value()
                .attr("name")
                .ok_or_else(|| unknown("hidden field has no name"))?;
            let value = input.value().attr("value").unwrap_or("");
            if hidden.len() >= 32
                || name.is_empty()
                || name.len() > 64
                || value.len() > 4096
                || name == password_name
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || hidden.iter().any(|(existing, _)| existing == name)
            {
                return Err(unknown("unrecognized management login token"));
            }
            hidden.push((name.to_owned(), value.to_owned()));
        }
        result = Some(LoginForm {
            password: name.to_owned(),
            hidden,
        });
    }
    result.ok_or_else(|| {
        Failure::new(
            FailureKind::Authentication,
            "management login form was not returned",
        )
    })
}

pub struct WebSession {
    client: Client,
    origin: String,
    fingerprint: Fingerprint,
    model: Model,
}

pub(crate) fn management_origin(host: &Host, port: u16) -> Result<String, Failure> {
    let url = reqwest::Url::parse(&format!("https://{}:{port}", host.authority()))
        .map_err(|_| Failure::new(FailureKind::InvalidInput, "invalid management origin"))?;
    Ok(url.origin().ascii_serialization())
}

impl WebSession {
    pub fn connect(profile: &Profile, timeout: Duration) -> Result<Self, Failure> {
        profile.validate(profile.printer.identity)?;
        let info = certificate_info(&profile.certificate_der)?;
        let tls_host = info
            .tls_host
            .as_ref()
            .unwrap_or_else(|| profile.endpoint.host());
        if *tls_host != profile.tls_host {
            return Err(Failure::new(
                FailureKind::Certificate,
                "profile TLS hostname differs from the certificate",
            ));
        }
        let addresses: Vec<_> = (profile.endpoint.host().as_str(), profile.web_port)
            .to_socket_addrs()
            .map_err(|_| {
                Failure::new(
                    FailureKind::Unreachable,
                    "unable to resolve the printer transport address",
                )
            })?
            .collect();
        if addresses.is_empty() {
            return Err(Failure::new(
                FailureKind::Unreachable,
                "printer transport address is unavailable",
            ));
        }
        let certificate = reqwest::Certificate::from_der(&profile.certificate_der)
            .map_err(|_| Failure::new(FailureKind::Certificate, "invalid trusted certificate"))?;
        let client_builder = builder(timeout)?;
        let client_builder = if info.tls_host.is_none() {
            client_builder.tls_backend_native()
        } else {
            client_builder
        };
        let client = client_builder
            .tls_certs_only([certificate])
            .tls_danger_accept_invalid_hostnames(info.tls_host.is_none())
            .cookie_store(true)
            .resolve_to_addrs(profile.tls_host.as_str(), &addresses)
            .build()
            .map_err(|_| {
                Failure::new(
                    FailureKind::Certificate,
                    "unable to initialize the trusted device connection",
                )
            })?;
        Ok(Self {
            client,
            origin: management_origin(&profile.tls_host, profile.web_port)?,
            fingerprint: profile.certificate_sha256,
            model: profile.printer.model,
        })
    }

    fn verify_peer(&self, response: &Response) -> Result<(), Failure> {
        let certificate = response
            .extensions()
            .get::<reqwest::tls::TlsInfo>()
            .and_then(|info| info.peer_certificate())
            .ok_or_else(|| {
                Failure::new(
                    FailureKind::Certificate,
                    "TLS peer certificate is unavailable",
                )
            })?;
        if Fingerprint::of(certificate) != self.fingerprint {
            return Err(Failure::new(
                FailureKind::Certificate,
                "device certificate changed",
            ));
        }
        Ok(())
    }

    fn document(&self, response: Response) -> Result<String, Failure> {
        self.verify_peer(&response)?;
        if matches!(
            response.status(),
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        ) {
            return Err(Failure::new(
                FailureKind::Authentication,
                "printer rejected the management credential",
            ));
        }
        if !response.status().is_success() {
            return Err(unknown(
                "printer returned an unsuccessful management HTTP status",
            ));
        }
        let document = String::from_utf8(bounded_body(response)?)
            .map_err(|_| unknown("management page is not UTF-8"))?;
        if page_model(&document)? != self.model {
            return Err(Failure::new(
                FailureKind::WrongDevice,
                "management page identifies another model",
            ));
        }
        Ok(document)
    }

    pub fn get(&self, path: &str) -> Result<String, Failure> {
        if !safe_path(path) {
            return Err(Failure::new(
                FailureKind::InvalidInput,
                "invalid management page path",
            ));
        }
        self.navigate(
            self.client
                .get(format!("{}{path}", self.origin))
                .send()
                .map_err(http_failure)?,
        )
    }

    fn navigate(&self, mut response: Response) -> Result<String, Failure> {
        for _ in 0..4 {
            self.verify_peer(&response)?;
            if !response.status().is_redirection() {
                return self.document(response);
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| unknown("management redirect has no location"))?;
            let target = navigation_url(response.url().as_str(), location)?;
            response = self.client.get(target).send().map_err(|error| {
                http_failure(error).at(crate::types::FailurePhase::LoginRedirect)
            })?;
        }
        Err(unknown("management redirect limit exceeded"))
    }

    pub fn login(&self, password: &Secret) -> Result<String, Failure> {
        let document = self
            .get(status_path(self.model))
            .map_err(|error| error.at(crate::types::FailurePhase::LoginPage))?;
        let form = login_form(&document)?;
        let mut fields: Vec<(&str, &str)> = form
            .hidden
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        fields.push((form.password_field(), password.expose()));
        let response = self
            .client
            .post(format!("{}{}", self.origin, status_path(self.model)))
            .header(reqwest::header::ORIGIN, &self.origin)
            .header(
                reqwest::header::REFERER,
                format!("{}{}", self.origin, status_path(self.model)),
            )
            .form(&fields)
            .send()
            .map_err(|error| http_failure(error).at(crate::types::FailurePhase::LoginSubmission))?;
        let document = self.navigate(response)?;
        let html = Html::parse_document(&document);
        if html
            .select(&selector("input[type=password]")?)
            .next()
            .is_some()
        {
            return Err(Failure::new(
                FailureKind::Authentication,
                "printer rejected the credential or requires an interactive password change",
            ));
        }
        Ok(document)
    }

    pub(crate) fn submit(
        &self,
        path: &str,
        fields: &[(String, String)],
    ) -> Result<String, Failure> {
        if !safe_path(path)
            || !(path.starts_with("/general/")
                || path.starts_with("/printer/")
                || path.starts_with("/print/")
                || path.starts_with("/admin/"))
            || fields.len() > 128
        {
            return Err(Failure::new(
                FailureKind::InvalidInput,
                "invalid management submission",
            ));
        }
        let response = self
            .client
            .post(format!("{}{path}", self.origin))
            .header(reqwest::header::ORIGIN, &self.origin)
            .header(reqwest::header::REFERER, format!("{}{path}", self.origin))
            .form(fields)
            .send()
            .map_err(|error| {
                if error.is_connect() {
                    http_failure(error)
                } else {
                    Failure::new(
                        FailureKind::OutcomeUnknown,
                        "management request may have been sent; it will not be retried",
                    )
                }
            })?;
        self.navigate(response).map_err(|failure| {
            if failure.kind == FailureKind::Authentication {
                failure
            } else {
                Failure::new(
                    FailureKind::OutcomeUnknown,
                    "management write was sent without a verifiable acknowledgement",
                )
            }
        })
    }
}

pub fn navigation_url(origin: &str, location: &str) -> Result<reqwest::Url, Failure> {
    let origin = reqwest::Url::parse(origin).map_err(|_| unknown("invalid management origin"))?;
    let mut target = origin
        .join(location)
        .map_err(|_| unknown("invalid management redirect"))?;
    if target.scheme() != "https"
        || origin.origin() != target.origin()
        || !target.username().is_empty()
        || target.password().is_some()
        || !safe_path(target.path())
    {
        return Err(Failure::new(
            FailureKind::Authentication,
            "management redirect leaves the trusted device origin",
        ));
    }
    target.set_fragment(None);
    Ok(target)
}

pub(crate) fn safe_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && path.len() <= 256
        && !path
            .split('/')
            .any(|component| component == "." || component == "..")
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/_-.".contains(&byte))
}
