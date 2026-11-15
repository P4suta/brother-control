use crate::{
    settings::SettingValue,
    types::{Failure, FailureKind, Model},
    web::{page_model, safe_path, selector},
};
use brother_core::{Duplex, Minutes, PaperSize, PaperType, Setting, SettingKey, Switch};
use scraper::{ElementRef, Html};

fn unknown() -> Failure {
    Failure::new(
        FailureKind::UnknownResponse,
        "unrecognized management settings form or value",
    )
}
fn text(element: ElementRef<'_>) -> String {
    element
        .text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn key(label: &str) -> Option<SettingKey> {
    match label.trim_end_matches(':') {
        "Sleep Time" => Some(SettingKey::Sleep),
        "Toner Save" => Some(SettingKey::TonerSave),
        "Quiet Mode" => Some(SettingKey::QuietMode),
        "2-sided Print" | "2-sided" | "Duplex" => Some(SettingKey::Duplex),
        "Paper Size" => Some(SettingKey::PaperSize),
        "Paper Type" => Some(SettingKey::PaperType),
        _ => None,
    }
}

fn value(key: SettingKey, label: &str) -> Result<SettingValue, Failure> {
    let switch = || match label {
        "Off" => Ok(Switch::Off),
        "On" => Ok(Switch::On),
        _ => Err(unknown()),
    };
    Ok(match key {
        SettingKey::Sleep => Setting::Sleep(
            Minutes::new(label.parse::<u8>().map_err(|_| unknown())?).map_err(|_| unknown())?,
        ),
        SettingKey::TonerSave => Setting::TonerSave(switch()?),
        SettingKey::QuietMode => Setting::QuietMode(switch()?),
        SettingKey::Duplex => Setting::Duplex(match label {
            "Off" => Duplex::Off,
            "Long Edge" | "Long Edge (Left)" => Duplex::LongEdge,
            "Short Edge" | "Short Edge (Top)" => Duplex::ShortEdge,
            _ => return Err(unknown()),
        }),
        SettingKey::PaperSize => Setting::PaperSize(match label {
            "A4" => PaperSize::A4,
            "A5" => PaperSize::A5,
            "A6" => PaperSize::A6,
            "Letter" => PaperSize::Letter,
            "Legal" => PaperSize::Legal,
            "Executive" => PaperSize::Executive,
            "B5" | "B5(JIS)" | "B5 (JIS)" => PaperSize::B5,
            "B6" | "B6(JIS)" | "B6 (JIS)" => PaperSize::B6,
            _ => return Err(unknown()),
        }),
        SettingKey::PaperType => Setting::PaperType(match label {
            "Plain Paper" | "Plain" => PaperType::Plain,
            "Thin Paper" | "Thin" => PaperType::Thin,
            "Thick Paper" | "Thick" => PaperType::Thick,
            "Thicker Paper" | "Thicker" => PaperType::Thicker,
            "Recycled Paper" | "Recycled" => PaperType::Recycled,
            "Bond Paper" | "Bond" => PaperType::Bond,
            "Label" | "Labels" => PaperType::Label,
            "Envelope" | "Envelopes" => PaperType::Envelope,
            _ => return Err(unknown()),
        }),
    }
    .into())
}

struct Choice {
    wire: String,
    value: SettingValue,
}
struct Binding {
    name: String,
    current: SettingValue,
    choices: Vec<Choice>,
}
struct Form {
    fields: Vec<(String, String)>,
}
pub struct SettingsPage {
    path: String,
    forms: Vec<Form>,
    bindings: Vec<(usize, Binding)>,
}
pub struct Submission {
    path: String,
    fields: Vec<(String, String)>,
}
impl Submission {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn fields(&self) -> &[(String, String)] {
        &self.fields
    }
}
impl SettingsPage {
    pub fn values(&self) -> Vec<SettingValue> {
        self.bindings
            .iter()
            .map(|(_, binding)| binding.current)
            .collect()
    }
    pub fn submission(&self, desired: SettingValue) -> Result<Submission, Failure> {
        let (form_index, binding) = self
            .bindings
            .iter()
            .find(|(_, binding)| binding.current.key() == desired.key())
            .ok_or_else(unknown)?;
        let wire = if desired.key() == SettingKey::Sleep {
            match desired.core() {
                Setting::Sleep(minutes) => minutes.get().to_string(),
                Setting::TonerSave(_)
                | Setting::QuietMode(_)
                | Setting::Duplex(_)
                | Setting::PaperSize(_)
                | Setting::PaperType(_) => return Err(unknown()),
            }
        } else {
            binding
                .choices
                .iter()
                .find(|choice| choice.value == desired)
                .ok_or_else(unknown)?
                .wire
                .clone()
        };
        let mut fields = self.forms[*form_index].fields.clone();
        fields.retain(|(name, _)| name != &binding.name);
        fields.push((binding.name.clone(), wire));
        Ok(Submission {
            path: self.path.clone(),
            fields,
        })
    }
}

fn name(input: ElementRef<'_>) -> Result<&str, Failure> {
    let name = input.value().attr("name").ok_or_else(unknown)?;
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(unknown());
    }
    Ok(name)
}

fn form_fields(form: ElementRef<'_>) -> Result<Vec<(String, String)>, Failure> {
    let mut fields = Vec::new();
    for input in form.select(&selector("input, select, textarea")?) {
        if input.value().attr("disabled").is_some() {
            continue;
        }
        let kind = input.value().attr("type").unwrap_or("text");
        if matches!(kind, "submit" | "reset" | "button") {
            continue;
        }
        if kind == "password" {
            return Err(Failure::new(
                FailureKind::Authentication,
                "management page requires authentication",
            ));
        }
        let name = name(input)?;
        if matches!(kind, "checkbox" | "radio") && input.value().attr("checked").is_none() {
            continue;
        }
        let current = if input.value().name() == "select" {
            if input.value().attr("multiple").is_some() {
                return Err(unknown());
            }
            let options = selector("option")?;
            let selected: Vec<_> = input
                .select(&options)
                .filter(|option| option.value().attr("selected").is_some())
                .collect();
            if selected.len() > 1 {
                return Err(unknown());
            }
            let option = selected
                .first()
                .copied()
                .or_else(|| input.select(&options).next())
                .ok_or_else(unknown)?;
            option.value().attr("value").ok_or_else(unknown)?.to_owned()
        } else if input.value().name() == "textarea" {
            text(input)
        } else if matches!(kind, "text" | "hidden" | "number" | "checkbox" | "radio") {
            input.value().attr("value").unwrap_or("").to_owned()
        } else {
            return Err(unknown());
        };
        if fields.len() >= 128 || current.len() > 4096 {
            return Err(unknown());
        }
        fields.push((name.to_owned(), current));
    }
    Ok(fields)
}

fn bind(key: SettingKey, controls: &[ElementRef<'_>], html: &Html) -> Result<Binding, Failure> {
    let first = *controls.first().ok_or_else(unknown)?;
    let name = name(first)?.to_owned();
    if controls
        .iter()
        .any(|input| input.value().attr("name") != Some(name.as_str()))
    {
        return Err(unknown());
    }
    if key == SettingKey::Sleep
        && controls.len() == 1
        && first.value().name() == "input"
        && matches!(
            first.value().attr("type").unwrap_or("text"),
            "text" | "number"
        )
    {
        return Ok(Binding {
            name,
            current: value(key, first.value().attr("value").ok_or_else(unknown)?)?,
            choices: Vec::new(),
        });
    }
    let mut choices = Vec::new();
    let mut current = None;
    if controls.len() == 1 && first.value().name() == "select" {
        let options = selector("option")?;
        let has_selected = first
            .select(&options)
            .any(|option| option.value().attr("selected").is_some());
        for (index, option) in first.select(&options).enumerate() {
            let selected =
                option.value().attr("selected").is_some() || (!has_selected && index == 0);
            let decoded = match value(key, &text(option)) {
                Ok(value) => value,
                Err(_) if !selected => continue,
                Err(failure) => return Err(failure),
            };
            if selected && current.replace(decoded).is_some() {
                return Err(unknown());
            }
            choices.push(Choice {
                wire: option.value().attr("value").ok_or_else(unknown)?.to_owned(),
                value: decoded,
            });
        }
    } else {
        for input in controls {
            if input.value().attr("type") != Some("radio") {
                return Err(unknown());
            }
            let parent = input.parent().and_then(ElementRef::wrap);
            let label =
                if let Some(parent) = parent.filter(|parent| parent.value().name() == "label") {
                    text(parent)
                } else {
                    let id = input.value().attr("id").ok_or_else(unknown)?;
                    let labels = selector("label[for]")?;
                    let label = html
                        .select(&labels)
                        .find(|label| label.value().attr("for") == Some(id))
                        .ok_or_else(unknown)?;
                    text(label)
                };
            let decoded = value(key, &label)?;
            if input.value().attr("checked").is_some() && current.replace(decoded).is_some() {
                return Err(unknown());
            }
            choices.push(Choice {
                wire: input.value().attr("value").ok_or_else(unknown)?.to_owned(),
                value: decoded,
            });
        }
    }
    if choices.len() > 128
        || choices.iter().enumerate().any(|(index, choice)| {
            choices[..index]
                .iter()
                .any(|previous| previous.wire == choice.wire || previous.value == choice.value)
        })
    {
        return Err(unknown());
    }
    Ok(Binding {
        name,
        current: current.ok_or_else(unknown)?,
        choices,
    })
}

pub fn parse_page(model: Model, path: &str, document: &str) -> Result<SettingsPage, Failure> {
    if !safe_path(path)
        || !(path.starts_with("/general/")
            || path.starts_with("/printer/")
            || path.starts_with("/print/"))
        || document.len() > crate::ipp_adapter::MAX_RESPONSE_BYTES
        || page_model(document)? != model
    {
        return Err(unknown());
    }
    let html = Html::parse_document(document);
    if html
        .select(&selector("input[type=password]")?)
        .next()
        .is_some()
    {
        return Err(Failure::new(
            FailureKind::Authentication,
            "settings page requires authentication",
        ));
    }
    let mut result = SettingsPage {
        path: path.to_owned(),
        forms: Vec::new(),
        bindings: Vec::new(),
    };
    let labels = selector("dt, th, label[for]")?;
    let fields = selector("input, select")?;
    for form in html.select(&selector("form")?) {
        let mut bindings: Vec<Binding> = Vec::new();
        for label in form.select(&labels) {
            let Some(key) = key(&text(label)) else {
                continue;
            };
            let controls: Vec<_> = if let Some(id) = label.value().attr("for") {
                form.select(&fields)
                    .filter(|input| input.value().attr("id") == Some(id))
                    .collect()
            } else {
                let container = if label.value().name() == "dt" {
                    let adjacent = label.next_siblings().filter_map(ElementRef::wrap).next();
                    match adjacent {
                        Some(tray)
                            if tray.value().name() == "dt"
                                && matches!(text(tray).as_str(), "Tray 1" | "Tray #1") =>
                        {
                            tray.next_siblings()
                                .filter_map(ElementRef::wrap)
                                .next()
                                .filter(|element| element.value().name() == "dd")
                        }
                        value => value.filter(|element| element.value().name() == "dd"),
                    }
                } else {
                    label
                        .parent()
                        .and_then(ElementRef::wrap)
                        .filter(|element| element.value().name() == "tr")
                }
                .ok_or_else(unknown)?;
                container
                    .select(&fields)
                    .filter(|input| {
                        input.value().attr("type") != Some("hidden")
                            && input.value().attr("disabled").is_none()
                    })
                    .collect()
            };
            let binding = bind(key, &controls, &html)?;
            if let Some(previous) = bindings
                .iter()
                .find(|previous| previous.current.key() == key)
            {
                if previous.name != binding.name || previous.current != binding.current {
                    return Err(unknown());
                }
            } else {
                bindings.push(binding);
            }
        }
        if bindings.is_empty() {
            continue;
        }
        if form.value().attr("action") != Some(path)
            || !form
                .value()
                .attr("method")
                .is_some_and(|method| method.eq_ignore_ascii_case("post"))
        {
            return Err(unknown());
        }
        let index = result.forms.len();
        let mut fields = form_fields(form)?;
        for label in form
            .select(&selector("dt")?)
            .filter(|label| text(*label) == "Eco Mode")
        {
            let container = label
                .next_siblings()
                .filter_map(ElementRef::wrap)
                .next()
                .filter(|element| element.value().name() == "dd")
                .ok_or_else(unknown)?;
            let controls: Vec<_> = container.select(&selector("input")?).collect();
            let preset_name = name(*controls.first().ok_or_else(unknown)?)?;
            if controls.len() != 2
                || controls.iter().any(|input| {
                    input.value().attr("type") != Some("radio")
                        || input.value().attr("name") != Some(preset_name)
                })
                || controls
                    .iter()
                    .filter(|input| input.value().attr("checked").is_some())
                    .count()
                    != 1
            {
                return Err(unknown());
            }
            fields.retain(|(name, _)| name != preset_name);
        }
        result.forms.push(Form { fields });
        for binding in bindings {
            if result
                .bindings
                .iter()
                .any(|(_, previous)| previous.current.key() == binding.current.key())
            {
                return Err(unknown());
            }
            result.bindings.push((index, binding));
        }
    }
    if result.bindings.is_empty() {
        return Err(unknown());
    }
    Ok(result)
}

pub fn settings_links(document: &str) -> Result<Vec<String>, Failure> {
    let html = Html::parse_document(document);
    let mut paths = std::collections::BTreeSet::new();
    for anchor in html.select(&selector("a[href]")?) {
        let Some(href) = anchor.value().attr("href") else {
            continue;
        };
        let Ok(url) = crate::web::navigation_url("https://device.invalid/home/status.html", href)
        else {
            continue;
        };
        if url.query().is_none()
            && matches!(
                url.path(),
                "/general/ecology.html"
                    | "/general/sleep.html"
                    | "/printer/print.html"
                    | "/printer/tray.html"
                    | "/printer/duplex.html"
                    | "/print/print.html"
                    | "/print/tray.html"
                    | "/print/duplex.html"
            )
        {
            paths.insert(url.path().to_owned());
        }
    }
    if paths.len() > 32 {
        return Err(unknown());
    }
    Ok(paths.into_iter().collect())
}
