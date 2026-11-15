use crate::{
    ipp_adapter::now,
    types::{Failure, FailureKind, Host, Observation, Reading, Source},
};
use brother_core::SupplyLevel;
use serde::{Deserialize, Serialize};
use snmp2::{Oid, SyncSession, Value};
use std::time::{Duration, Instant};

const SUPPLY_BASE: [u64; 11] = [1, 3, 6, 1, 2, 1, 43, 11, 1, 1, 9];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Remaining {
    Percent(u8),
    SomeRemaining,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnmpState {
    Idle,
    Printing,
    Warming,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Supply {
    pub table_index: Vec<u64>,
    pub description: Reading<String>,
    pub remaining: Reading<Remaining>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnmpStatus {
    pub description: Reading<String>,
    pub state: Reading<SnmpState>,
    pub supplies: Vec<Supply>,
    pub supply_failure: Option<Failure>,
}

pub fn remaining(level: i64, capacity: i64) -> Reading<Remaining> {
    match SupplyLevel::from_snmp(level, capacity) {
        SupplyLevel::Percent(value) => Reading::Known {
            value: Remaining::Percent(value),
        },
        SupplyLevel::SomeRemaining => Reading::Known {
            value: Remaining::SomeRemaining,
        },
        SupplyLevel::Other => Reading::Unknown {
            reason: "SNMP reported other level (-1)".to_owned(),
        },
        SupplyLevel::Unknown => Reading::Unknown {
            reason: "SNMP level or capacity is unknown (-2)".to_owned(),
        },
        SupplyLevel::Invalid => Reading::Failed {
            failure: Failure::new(
                FailureKind::UnknownResponse,
                "SNMP supply level is outside the declared capacity",
            ),
        },
    }
}

enum Scalar {
    Number(i64),
    Text(String),
    Unsupported,
    Missing,
    End,
    Unknown,
}
enum ReadOperation {
    Get,
    Next,
}

struct Reader<'a> {
    host: &'a Host,
    deadline: Instant,
}

impl Reader<'_> {
    fn read(&self, arcs: &[u64], operation: ReadOperation) -> Result<(Vec<u64>, Scalar), Failure> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Failure::new(
                FailureKind::Timeout,
                "SNMP operation deadline expired",
            ));
        }
        let oid = Oid::from(arcs).map_err(|_| {
            Failure::new(FailureKind::InvalidInput, "invalid SNMP object identifier")
        })?;
        let mut session = SyncSession::new_v2c(
            (self.host.as_str(), 161),
            b"public",
            Some(remaining.min(Duration::from_secs(1))),
            1,
        )
        .map_err(|_| Failure::new(FailureKind::Unreachable, "unable to open the SNMP session"))?;
        let response = match operation {
            ReadOperation::Get => session.get(&oid),
            ReadOperation::Next => session.getnext(&oid),
        }
        .map_err(|_| {
            Failure::new(
                FailureKind::Unreachable,
                "SNMP request received no valid response",
            )
        })?;
        if response.error_status != 0 {
            return Err(Failure::new(
                FailureKind::UnknownResponse,
                "printer rejected the SNMP query",
            ));
        }
        let mut bindings = response.varbinds;
        let (name, value) = bindings.next().ok_or_else(|| {
            Failure::new(
                FailureKind::UnknownResponse,
                "SNMP response contains no value",
            )
        })?;
        if bindings.next().is_some() {
            return Err(Failure::new(
                FailureKind::UnknownResponse,
                "SNMP response contains unexpected values",
            ));
        }
        let name = name
            .iter()
            .ok_or_else(|| {
                Failure::new(
                    FailureKind::UnknownResponse,
                    "SNMP object identifier is too large",
                )
            })?
            .collect();
        let value = match value {
            Value::Integer(value) => Scalar::Number(value),
            Value::Unsigned32(value) | Value::Counter32(value) => Scalar::Number(i64::from(value)),
            Value::OctetString(value) => match std::str::from_utf8(value) {
                Ok(value) => Scalar::Text(value.to_owned()),
                Err(_) => Scalar::Unknown,
            },
            Value::NoSuchObject => Scalar::Unsupported,
            Value::NoSuchInstance => Scalar::Missing,
            Value::EndOfMibView => Scalar::End,
            Value::Boolean(_)
            | Value::Null
            | Value::ObjectIdentifier(_)
            | Value::Sequence(_)
            | Value::Set(_)
            | Value::Constructed(_, _)
            | Value::IpAddress(_)
            | Value::Timeticks(_)
            | Value::Opaque(_)
            | Value::Counter64(_)
            | Value::GetRequest(_)
            | Value::GetNextRequest(_)
            | Value::GetBulkRequest(_)
            | Value::Response(_)
            | Value::SetRequest(_)
            | Value::InformRequest(_)
            | Value::Trap(_)
            | Value::Report(_) => Scalar::Unknown,
        };
        Ok((name, value))
    }

    fn get(&self, arcs: &[u64]) -> Result<Scalar, Failure> {
        let (name, value) = self.read(arcs, ReadOperation::Get)?;
        if name != arcs {
            return Err(Failure::new(
                FailureKind::UnknownResponse,
                "SNMP response identifies another object",
            ));
        }
        Ok(value)
    }

    fn string(&self, arcs: &[u64]) -> Reading<String> {
        match self.get(arcs) {
            Ok(Scalar::Text(value)) => Reading::Known { value },
            Ok(Scalar::Unsupported) => Reading::Unsupported {
                reason: "SNMP object is unsupported".to_owned(),
            },
            Ok(Scalar::Missing) | Ok(Scalar::End) => Reading::Missing {
                reason: "SNMP instance was not returned".to_owned(),
            },
            Ok(Scalar::Number(_)) | Ok(Scalar::Unknown) => Reading::Unknown {
                reason: "SNMP object is not text".to_owned(),
            },
            Err(failure) => Reading::Failed { failure },
        }
    }

    fn supplies(&self) -> (Vec<Supply>, Option<Failure>) {
        let mut supplies = Vec::new();
        let mut cursor = SUPPLY_BASE.to_vec();
        for _ in 0..64 {
            let (name, value) = match self.read(&cursor, ReadOperation::Next) {
                Ok(value) => value,
                Err(error) => return (supplies, Some(error)),
            };
            if matches!(value, Scalar::End | Scalar::Unsupported | Scalar::Missing)
                || !name.starts_with(&SUPPLY_BASE)
            {
                return (supplies, None);
            }
            if name <= cursor || name.len() <= SUPPLY_BASE.len() {
                return (
                    supplies,
                    Some(Failure::new(
                        FailureKind::UnknownResponse,
                        "SNMP supply walk did not advance",
                    )),
                );
            }
            let index = name[SUPPLY_BASE.len()..].to_vec();
            let column = |number: u64| {
                let mut oid = SUPPLY_BASE.to_vec();
                oid.pop();
                oid.push(number);
                oid.extend_from_slice(&index);
                oid
            };
            let level = match value {
                Scalar::Number(level) => match self.get(&column(8)) {
                    Ok(Scalar::Number(capacity)) => remaining(level, capacity),
                    Ok(
                        Scalar::Text(_)
                        | Scalar::Unsupported
                        | Scalar::Missing
                        | Scalar::End
                        | Scalar::Unknown,
                    ) => Reading::Unknown {
                        reason: "SNMP capacity was not available".to_owned(),
                    },
                    Err(failure) => Reading::Failed { failure },
                },
                Scalar::Text(_)
                | Scalar::Unsupported
                | Scalar::Missing
                | Scalar::End
                | Scalar::Unknown => Reading::Unknown {
                    reason: "SNMP level was not numeric".to_owned(),
                },
            };
            supplies.push(Supply {
                description: self.string(&column(6)),
                remaining: level,
                table_index: index,
            });
            cursor = name;
        }
        (
            supplies,
            Some(Failure::new(
                FailureKind::UnknownResponse,
                "SNMP supply table exceeds the row limit",
            )),
        )
    }
}

pub fn query(host: &Host, timeout: Duration) -> Result<Observation<SnmpStatus>, Failure> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| Failure::new(FailureKind::InvalidInput, "invalid SNMP deadline"))?;
    let reader = Reader { host, deadline };
    let description = reader.string(&[1, 3, 6, 1, 2, 1, 1, 1, 0]);
    let state = match reader.get(&[1, 3, 6, 1, 2, 1, 25, 3, 5, 1, 1, 1]) {
        Ok(Scalar::Number(3)) => Reading::Known {
            value: SnmpState::Idle,
        },
        Ok(Scalar::Number(4)) => Reading::Known {
            value: SnmpState::Printing,
        },
        Ok(Scalar::Number(5)) => Reading::Known {
            value: SnmpState::Warming,
        },
        Ok(Scalar::Number(_)) | Ok(Scalar::Unknown) | Ok(Scalar::Text(_)) => Reading::Unknown {
            reason: "SNMP printer status is unknown".to_owned(),
        },
        Ok(Scalar::Unsupported) => Reading::Unsupported {
            reason: "SNMP printer status is unsupported".to_owned(),
        },
        Ok(Scalar::Missing) | Ok(Scalar::End) => Reading::Missing {
            reason: "SNMP printer status was not returned".to_owned(),
        },
        Err(failure) => Reading::Failed { failure },
    };
    let (supplies, supply_failure) = reader.supplies();
    Ok(Observation {
        source: Source::Snmp,
        observed_at_unix_seconds: now()?,
        reading: Reading::Known {
            value: SnmpStatus {
                description,
                state,
                supplies,
                supply_failure,
            },
        },
    })
}
