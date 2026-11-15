use crate::{
    ipp_adapter::{Endpoint, now},
    types::{Failure, FailureKind, Host, Identity, Model, Printer, Reading, Source},
};
use hickory_proto::{
    op::{Message, Query},
    rr::{Name, RData, RecordType},
    serialize::binary::BinEncodable,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, UdpSocket},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveredPrinter {
    pub printer: Printer,
    pub endpoint: Endpoint,
    pub addresses: Vec<IpAddr>,
    pub source: Source,
    pub observed_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryReport {
    pub printers: Vec<DiscoveredPrinter>,
    pub failures: Vec<Failure>,
    pub attempts: Vec<ProbeAttempt>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeAttempt {
    pub endpoint: Endpoint,
    pub source: Source,
    pub observed_at_unix_seconds: u64,
    pub reading: Reading<Printer>,
}

#[derive(Clone, Debug)]
pub struct PrivateSubnet {
    network: u32,
    prefix: u8,
}

impl PrivateSubnet {
    pub fn parse(input: &str) -> Result<Self, Failure> {
        let invalid = || {
            Failure::new(
                FailureKind::InvalidInput,
                "expected a canonical private IPv4 subnet with prefix 24 through 32",
            )
        };
        let (network, prefix) = input.split_once('/').ok_or_else(invalid)?;
        let network = network.parse::<Ipv4Addr>().map_err(|_| invalid())?;
        let prefix = prefix.parse::<u8>().map_err(|_| invalid())?;
        if !(24..=32).contains(&prefix) {
            return Err(invalid());
        }
        let mask = u32::MAX << (32 - prefix);
        let value = u32::from(network);
        if value & mask != value
            || !network.is_private()
            || !Ipv4Addr::from(value | !mask).is_private()
        {
            return Err(invalid());
        }
        Ok(Self {
            network: value,
            prefix,
        })
    }

    pub fn hosts(&self, site: Option<u16>) -> Result<Vec<Host>, Failure> {
        let count = 1_u32 << (32 - self.prefix);
        let skip = u32::from(self.prefix < 31);
        (skip..count - skip)
            .map(|offset| {
                let address = self.network + offset;
                let host = match site {
                    None => Ipv4Addr::from(address).to_string(),
                    Some(site) => Ipv6Addr::new(
                        0xfd7a,
                        0x115c,
                        0xa1e0,
                        0xb1a,
                        0,
                        site,
                        (address >> 16) as u16,
                        address as u16,
                    )
                    .to_string(),
                };
                Host::parse(&host)
            })
            .collect()
    }
}

impl std::str::FromStr for PrivateSubnet {
    type Err = Failure;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

pub fn probe(endpoints: &[Endpoint], window: Duration) -> Result<DiscoveryReport, Failure> {
    if endpoints.len() > 256 || window.is_zero() || window > Duration::from_secs(30) {
        return Err(Failure::new(
            FailureKind::InvalidInput,
            "probe at most 256 targets within 1 to 30000 milliseconds",
        ));
    }
    let deadline = Instant::now()
        .checked_add(window)
        .ok_or_else(|| malformed("probe deadline overflow"))?;
    let at = now()?;
    let mut groups = [
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    ];
    thread::scope(|scope| {
        for (worker, results) in groups.iter_mut().enumerate() {
            let _worker = scope.spawn(move || {
                for endpoint in endpoints.iter().skip(worker).step_by(8) {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    let result = if remaining.is_zero() {
                        Err(Failure::new(
                            FailureKind::Timeout,
                            "probe deadline elapsed before the target was queried",
                        ))
                    } else {
                        crate::ipp_adapter::query(
                            endpoint,
                            remaining.min(Duration::from_millis(500)),
                        )
                    };
                    results.push((endpoint.clone(), result));
                }
            });
        }
    });
    let mut printers = BTreeMap::new();
    let mut attempts = Vec::new();
    for (endpoint, result) in groups.into_iter().flatten() {
        let (observed_at_unix_seconds, reading) = match result {
            Ok(status) => {
                let at = status.state.observed_at_unix_seconds;
                let printer = DiscoveredPrinter {
                    observed_at_unix_seconds: at,
                    printer: status.printer.clone(),
                    endpoint: endpoint.clone(),
                    addresses: endpoint
                        .host()
                        .as_str()
                        .parse::<IpAddr>()
                        .into_iter()
                        .collect(),
                    source: Source::Ipp,
                };
                printers.insert(printer.printer.identity.to_string(), printer);
                (
                    at,
                    Reading::Known {
                        value: status.printer,
                    },
                )
            }
            Err(failure) => (at, Reading::Failed { failure }),
        };
        attempts.push(ProbeAttempt {
            endpoint,
            source: Source::Ipp,
            observed_at_unix_seconds,
            reading,
        });
    }
    Ok(DiscoveryReport {
        printers: printers.into_values().collect(),
        failures: Vec::new(),
        attempts,
    })
}

fn malformed(reason: &str) -> Failure {
    Failure::new(FailureKind::UnknownResponse, reason)
}

pub fn txt_properties(mut bytes: &[u8]) -> Result<BTreeMap<String, String>, Failure> {
    let mut properties = BTreeMap::new();
    while let Some((&length, remaining)) = bytes.split_first() {
        let length = usize::from(length);
        let value = remaining
            .get(..length)
            .ok_or_else(|| malformed("truncated DNS-SD TXT field"))?;
        bytes = remaining
            .get(length..)
            .ok_or_else(|| malformed("truncated DNS-SD TXT field"))?;
        if value.is_empty() {
            continue;
        }
        let value =
            std::str::from_utf8(value).map_err(|_| malformed("non-UTF-8 DNS-SD TXT field"))?;
        let (key, value) = value.split_once('=').unwrap_or((value, ""));
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && byte != b'=')
        {
            return Err(malformed("invalid DNS-SD TXT key"));
        }
        properties
            .entry(key.to_ascii_lowercase())
            .or_insert_with(|| value.to_owned());
    }
    Ok(properties)
}

pub fn decode_packet(bytes: &[u8], at: u64) -> Result<Vec<DiscoveredPrinter>, Failure> {
    let message = Message::from_vec(bytes).map_err(|_| malformed("malformed DNS-SD packet"))?;
    if message.answers.len() + message.additionals.len() > 1024 {
        return Err(malformed("too many DNS-SD records"));
    }
    let records: Vec<_> = message.answers.iter().chain(&message.additionals).collect();
    let mut printers = Vec::new();
    for record in &records {
        let RData::SRV(service) = &record.data else {
            continue;
        };
        if record.ttl == 0
            || !record
                .name
                .to_ascii()
                .to_ascii_lowercase()
                .ends_with("._ipp._tcp.local.")
        {
            continue;
        }
        let mut properties = BTreeMap::new();
        for candidate in &records {
            if candidate.name == record.name
                && candidate.ttl > 0
                && let RData::TXT(text) = &candidate.data
            {
                properties = txt_properties(
                    &text
                        .to_bytes()
                        .map_err(|_| malformed("unable to decode DNS-SD TXT record"))?,
                )?;
                break;
            }
        }
        let Some(model) = properties
            .get("ty")
            .and_then(|value| Model::parse(value).ok())
        else {
            continue;
        };
        let identity = Identity::parse(
            properties
                .get("uuid")
                .ok_or_else(|| malformed("supported printer has no DNS-SD UUID"))?,
        )?;
        let resource = properties
            .get("rp")
            .ok_or_else(|| malformed("supported printer has no IPP resource"))?;
        let resource = if resource.starts_with('/') {
            resource.to_owned()
        } else {
            format!("/{resource}")
        };
        let host = Host::parse(&service.target.to_ascii())?;
        let endpoint = Endpoint::new(host.clone(), service.port, &resource)?;
        let mut addresses = Vec::new();
        for address in &records {
            if address.name != service.target || address.ttl == 0 {
                continue;
            }
            if let RData::A(value) = &address.data {
                addresses.push(IpAddr::V4(value.0));
            }
            if let RData::AAAA(value) = &address.data {
                addresses.push(IpAddr::V6(value.0));
            }
        }
        addresses.sort();
        addresses.dedup();
        printers.push(DiscoveredPrinter {
            printer: Printer {
                identity,
                model,
                host,
            },
            endpoint,
            addresses,
            source: Source::DnsSd,
            observed_at_unix_seconds: at,
        });
    }
    Ok(printers)
}

pub fn discover(window: Duration) -> Result<DiscoveryReport, Failure> {
    if window.is_zero() || window > Duration::from_secs(30) {
        return Err(Failure::new(
            FailureKind::InvalidInput,
            "discovery duration must be at most 30 seconds and greater than zero",
        ));
    }
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|_| {
        Failure::new(
            FailureKind::Unreachable,
            "unable to open the discovery socket",
        )
    })?;
    socket
        .set_multicast_ttl_v4(255)
        .map_err(|_| Failure::new(FailureKind::Unreachable, "unable to set the multicast TTL"))?;
    let mut query = Message::query();
    query.add_query(Query::query(
        Name::from_ascii("_ipp._tcp.local.")
            .map_err(|_| malformed("invalid discovery service name"))?,
        RecordType::PTR,
    ));
    socket
        .send_to(
            &query
                .to_vec()
                .map_err(|_| malformed("unable to encode the discovery query"))?,
            (Ipv4Addr::new(224, 0, 0, 251), 5353),
        )
        .map_err(|_| {
            Failure::new(
                FailureKind::Unreachable,
                "unable to send the discovery query",
            )
        })?;
    let deadline = Instant::now()
        .checked_add(window)
        .ok_or_else(|| malformed("discovery deadline overflow"))?;
    let mut found = BTreeMap::new();
    let mut failures = Vec::new();
    let mut buffer = [0_u8; 65_535];
    for _ in 0..256 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        socket.set_read_timeout(Some(remaining)).map_err(|_| {
            Failure::new(FailureKind::Storage, "unable to set the discovery deadline")
        })?;
        let (length, peer) = match socket.recv_from(&mut buffer) {
            Ok(value) => value,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                break;
            }
            Err(_) => {
                return Err(Failure::new(
                    FailureKind::Unreachable,
                    "discovery socket failed",
                ));
            }
        };
        if peer.port() != 5353 {
            continue;
        }
        match decode_packet(&buffer[..length], now()?) {
            Ok(printers) => {
                for printer in printers {
                    found.insert(printer.printer.identity.to_string(), printer);
                }
            }
            Err(error) => failures.push(error),
        }
    }
    Ok(DiscoveryReport {
        printers: found.into_values().collect(),
        failures,
        attempts: Vec::new(),
    })
}
