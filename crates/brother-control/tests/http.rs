use brother_control::{
    ipp_adapter::{Endpoint, query},
    types::{Failure, FailureKind, Host},
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

fn io_failure(_: std::io::Error) -> Failure {
    Failure::new(FailureKind::Storage, "test HTTP transport failed")
}

#[test]
fn transport_sends_only_an_attribute_request_and_preserves_http_failures() -> Result<(), Failure> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(io_failure)?;
    let endpoint = Endpoint::new(
        Host::parse("127.0.0.1")?,
        listener.local_addr().map_err(io_failure)?.port(),
        "/ipp/print",
    )?;
    let mut captured = None;
    let result = thread::scope(|scope| {
        let _worker = scope.spawn(|| {
            captured = Some(serve(listener));
        });
        query(&endpoint, Duration::from_secs(2))
    });
    let payload = captured
        .ok_or_else(|| Failure::new(FailureKind::Storage, "test server produced no request"))?
        .map_err(io_failure)?;
    assert_eq!(payload.get(2..4), Some([0, 11].as_slice()));
    assert_eq!(
        result.map(|_| ()).map_err(|error| error.kind),
        Err(FailureKind::Authentication)
    );
    Ok(())
}

fn serve(listener: TcpListener) -> std::io::Result<Vec<u8>> {
    let (mut connection, _) = listener.accept()?;
    connection.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut input = BufReader::new(connection.try_clone()?);
    let mut first = String::new();
    input.read_line(&mut first)?;
    assert_eq!(first, "POST /ipp/print HTTP/1.1\r\n");
    let mut length = 0;
    loop {
        let mut line = String::new();
        input.read_line(&mut line)?;
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value
                .trim()
                .parse::<usize>()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        }
    }
    assert!(length > 0 && length < 65_536);
    let mut payload = vec![0; length];
    input.read_exact(&mut payload)?;
    connection
        .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
    Ok(payload)
}
