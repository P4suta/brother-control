use crate::{
    config::ConfigStore,
    types::{Failure, FailureKind, Identity},
    web::{WebSession, navigation_url, selector},
};
use scraper::{ElementRef, Html};
use serde::Serialize;
use std::time::Duration;

#[derive(Serialize)]
pub struct Control {
    tag: String,
    kind: Option<String>,
    name: Option<String>,
    id: Option<String>,
    options: Vec<OptionLabel>,
}
#[derive(Serialize)]
pub struct OptionLabel {
    label: String,
    selected: bool,
}
#[derive(Serialize)]
pub struct Form {
    action: Option<String>,
    method: Option<String>,
    controls: Vec<Control>,
}
#[derive(Serialize)]
pub struct Link {
    label: String,
    path: String,
}
#[derive(Serialize)]
pub struct Structure {
    headings: Vec<String>,
    labels: Vec<String>,
    forms: Vec<Form>,
    links: Vec<Link>,
    adjacent_controls: Vec<LabelControls>,
}

#[derive(Serialize)]
struct LabelControls {
    label: String,
    adjacent_tag: Option<String>,
    control_names: Vec<String>,
}

fn text(element: ElementRef<'_>) -> String {
    element
        .text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(256)
        .collect()
}
fn path(href: &str) -> Option<String> {
    navigation_url("https://device.invalid/home/status.html", href)
        .ok()
        .map(|url| url.path().to_owned())
}

pub fn structure(document: &str) -> Result<Structure, Failure> {
    if document.len() > crate::ipp_adapter::MAX_RESPONSE_BYTES {
        return Err(Failure::new(
            FailureKind::UnknownResponse,
            "management page exceeds the inspection limit",
        ));
    }
    let html = Html::parse_document(document);
    let headings = html.select(&selector("h1, h2, h3")?).map(text).collect();
    let labels = html.select(&selector("dt, th, label")?).map(text).collect();
    let mut forms = Vec::new();
    for form in html.select(&selector("form")?) {
        let mut controls = Vec::new();
        for input in form.select(&selector("input, select, textarea")?).take(128) {
            let options = input
                .select(&selector("option")?)
                .take(128)
                .map(|option| OptionLabel {
                    label: text(option),
                    selected: option.value().attr("selected").is_some(),
                })
                .collect();
            controls.push(Control {
                tag: input.value().name().to_owned(),
                kind: input.value().attr("type").map(str::to_owned),
                name: input.value().attr("name").map(str::to_owned),
                id: input.value().attr("id").map(str::to_owned),
                options,
            });
        }
        forms.push(Form {
            action: form.value().attr("action").and_then(path),
            method: form.value().attr("method").map(str::to_owned),
            controls,
        });
    }
    let links = html
        .select(&selector("a[href]")?)
        .filter_map(|anchor| {
            anchor.value().attr("href").and_then(path).map(|path| Link {
                label: text(anchor),
                path,
            })
        })
        .take(128)
        .collect();
    let mut adjacent_controls = Vec::new();
    for label in html.select(&selector("dt, th")?).take(128) {
        let adjacent = label.next_siblings().filter_map(ElementRef::wrap).next();
        let control_names = match adjacent {
            Some(element) => element
                .select(&selector("input, select")?)
                .filter_map(|input| input.value().attr("name").map(str::to_owned))
                .collect(),
            None => Vec::new(),
        };
        adjacent_controls.push(LabelControls {
            label: text(label),
            adjacent_tag: adjacent.map(|element| element.value().name().to_owned()),
            control_names,
        });
    }
    Ok(Structure {
        headings,
        labels,
        forms,
        links,
        adjacent_controls,
    })
}

pub fn inspect(
    store: &ConfigStore,
    device: Identity,
    page: &str,
    timeout: Duration,
) -> Result<Structure, Failure> {
    let profile = store.load(device)?;
    let _lease = store.lease(device)?;
    let password = profile.credential.load(device)?;
    let session = WebSession::connect(&profile, timeout)?;
    session.login(&password)?;
    structure(&session.get(page)?)
}
