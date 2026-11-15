use crate::{
    config::{CredentialRef, Fingerprint, Profile},
    ipp_adapter::Endpoint,
    types::{Host, Identity, Model, Printer},
    web::WebSession,
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

#[test]
fn management_origins_omit_default_https_ports() -> Result<(), String> {
    let host = Host::parse("printer.local").map_err(|error| error.to_string())?;
    assert_eq!(
        crate::web::management_origin(&host, 443).map_err(|error| error.to_string())?,
        "https://printer.local"
    );
    assert_eq!(
        crate::web::management_origin(&host, 8443).map_err(|error| error.to_string())?,
        "https://printer.local:8443"
    );
    Ok(())
}

fn fixture_profile(certificate: &[u8], port: u16) -> Result<Profile, String> {
    let host = Host::parse("127.0.0.1").map_err(|error| error.to_string())?;
    let identity = Identity::parse("11111111-1111-4111-8111-111111111111")
        .map_err(|error| error.to_string())?;
    Ok(Profile {
        schema: 1,
        printer: Printer {
            identity,
            model: Model::HlL2375dw,
            host: host.clone(),
        },
        endpoint: Endpoint::new(host.clone(), 631, "/ipp/print")
            .map_err(|error| error.to_string())?,
        tls_host: host,
        web_port: port,
        certificate_der: certificate.to_vec(),
        certificate_sha256: Fingerprint::of(certificate),
        credential: CredentialRef::environment(identity),
    })
}

#[test]
fn registered_legacy_roots_accept_only_the_selected_printer_key() -> Result<(), String> {
    for trusted in [true, false] {
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_RSA_SHA256)
            .map_err(|error| error.to_string())?;
        let certificate = rcgen::CertificateParams::new(Vec::<String>::new())
            .map_err(|error| error.to_string())?
            .self_signed(&key)
            .map_err(|error| error.to_string())?;
        let unrelated = rcgen::generate_simple_self_signed(Vec::<String>::new())
            .map_err(|error| error.to_string())?;
        let identity = native_tls::Identity::from_pkcs8(
            certificate.pem().as_bytes(),
            key.serialize_pem().as_bytes(),
        )
        .map_err(|error| error.to_string())?;
        let acceptor = native_tls::TlsAcceptor::new(identity).map_err(|error| error.to_string())?;
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let profile = fixture_profile(
            if trusted {
                certificate.der().as_ref()
            } else {
                unrelated.cert.der().as_ref()
            },
            listener
                .local_addr()
                .map_err(|error| error.to_string())?
                .port(),
        )?;
        let mut received_http = false;
        let mut server_result = None;
        let result = thread::scope(|scope| {
            let _server = scope.spawn(|| {
                server_result = Some((|| -> Result<(), String> {
                    let (stream, _) = listener.accept().map_err(|error| error.to_string())?;
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .map_err(|error| error.to_string())?;
                    stream
                        .set_write_timeout(Some(Duration::from_secs(3)))
                        .map_err(|error| error.to_string())?;
                    let Ok(mut tls) = acceptor.accept(stream) else {
                        return Ok(());
                    };
                    let mut bytes = [0; 4096];
                    let count = match tls.read(&mut bytes) {
                        Ok(count) => count,
                        Err(error)
                            if !trusted
                                && matches!(
                                    error.kind(),
                                    std::io::ErrorKind::ConnectionReset
                                        | std::io::ErrorKind::BrokenPipe
                                        | std::io::ErrorKind::UnexpectedEof
                                ) =>
                        {
                            return Ok(());
                        }
                        Err(error) => {
                            return Err(format!(
                                "TLS fixture read failed (trusted={trusted}): {error}"
                            ));
                        }
                    };
                    if count > 0 {
                        received_http = true;
                        let body = "<title>Brother HL-L2375DW</title>";
                        write!(
                        tls,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                })());
            });
            WebSession::connect(&profile, Duration::from_secs(3))
                .and_then(|session| session.get("/general/status.html"))
        });
        server_result.ok_or_else(|| "HTTPS fixture did not finish".to_owned())??;
        assert_eq!(result.is_ok(), trusted);
        assert_eq!(received_http, trusted);
    }
    Ok(())
}

#[test]
fn form_submissions_identify_the_authenticated_origin_and_source_page() -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    let key =
        rcgen::KeyPair::generate_for(&rcgen::PKCS_RSA_SHA256).map_err(|error| error.to_string())?;
    let certificate = rcgen::CertificateParams::new(Vec::<String>::new())
        .map_err(|error| error.to_string())?
        .self_signed(&key)
        .map_err(|error| error.to_string())?;
    let identity = native_tls::Identity::from_pkcs8(
        certificate.pem().as_bytes(),
        key.serialize_pem().as_bytes(),
    )
    .map_err(|error| error.to_string())?;
    let acceptor = native_tls::TlsAcceptor::new(identity).map_err(|error| error.to_string())?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let profile = fixture_profile(certificate.der().as_ref(), port)?;
    let mut server_result = None;
    let result = thread::scope(|scope| {
        let _server = scope.spawn(|| {
            server_result = Some((|| -> Result<bool, String> {
                let (stream, _) = listener.accept().map_err(|error| error.to_string())?;
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .map_err(|error| error.to_string())?;
                stream
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .map_err(|error| error.to_string())?;
                let mut tls = acceptor.accept(stream).map_err(|error| error.to_string())?;
                let mut input = BufReader::new(&mut tls);
                let mut origin = false;
                let mut referer = false;
                let mut length = 0_usize;
                for _ in 0..64 {
                    let mut line = String::new();
                    if input
                        .read_line(&mut line)
                        .map_err(|error| error.to_string())?
                        == 0
                    {
                        break;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value
                                .trim()
                                .parse()
                                .map_err(|_| "invalid fixture body length".to_owned())?;
                        }
                        origin |= name.eq_ignore_ascii_case("origin")
                            && value.trim() == format!("https://127.0.0.1:{port}");
                        referer |= name.eq_ignore_ascii_case("referer")
                            && value.trim()
                                == format!("https://127.0.0.1:{port}/general/sleep.html");
                    }
                }
                if length > 256 {
                    return Err("fixture body exceeds limit".to_owned());
                }
                let mut body = vec![0; length];
                input
                    .read_exact(&mut body)
                    .map_err(|error| error.to_string())?;
                drop(input);
                let status = if origin && referer {
                    "200 OK"
                } else {
                    "403 Forbidden"
                };
                let body = "<title>Brother HL-L2375DW</title>";
                write!(
                    tls,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .map_err(|error| error.to_string())?;
                Ok(origin && referer)
            })());
        });
        WebSession::connect(&profile, Duration::from_secs(3)).and_then(|session| {
            session.submit(
                "/general/sleep.html",
                &Vec::from([("B1b".to_owned(), "2".to_owned())]),
            )
        })
    });
    assert!(server_result.ok_or_else(|| "submission fixture did not finish".to_owned())??);
    assert!(result.is_ok());
    Ok(())
}
