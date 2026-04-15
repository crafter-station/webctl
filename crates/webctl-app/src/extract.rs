use std::collections::BTreeMap;

use scraper::{Html, Selector};
use webctl_ir::{
    CssFieldSource, ExtractedItem, ExtractedValue, Extractor, FieldDef, FieldSource, FieldType,
    ListExtractor,
};

pub fn extract_items(html: &str, extractor: &Extractor) -> Option<Vec<ExtractedItem>> {
    match extractor {
        Extractor::List(list) => extract_list(html, list),
        Extractor::Detail(_detail) => None,
        Extractor::Raw => None,
    }
}

fn extract_list(html: &str, list: &ListExtractor) -> Option<Vec<ExtractedItem>> {
    let css = list.item_pattern.css_selector.as_deref()?;
    let doc = Html::parse_document(html);
    let item_sel = Selector::parse(css).ok()?;

    let items: Vec<ExtractedItem> = doc
        .select(&item_sel)
        .enumerate()
        .map(|(i, element)| {
            let mut fields = BTreeMap::new();

            for field_def in &list.fields {
                let value = extract_field(&doc, &element, field_def);
                fields.insert(field_def.name.clone(), value);
            }

            ExtractedItem {
                index: i + 1,
                fields,
            }
        })
        .filter(|item| {
            item.fields.values().any(|v| !matches!(v, ExtractedValue::Missing))
        })
        .collect();

    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

fn extract_field(
    _doc: &Html,
    element: &scraper::ElementRef,
    field_def: &FieldDef,
) -> ExtractedValue {
    match &field_def.source {
        FieldSource::Css(css) => extract_css_field(element, css, &field_def.field_type),
        FieldSource::AxTree(_) => ExtractedValue::Missing,
    }
}

fn extract_css_field(
    element: &scraper::ElementRef,
    source: &CssFieldSource,
    field_type: &FieldType,
) -> ExtractedValue {
    let sel = match Selector::parse(&source.selector) {
        Ok(s) => s,
        Err(_) => return ExtractedValue::Missing,
    };

    let target = element.select(&sel).next();
    let Some(target) = target else {
        return ExtractedValue::Missing;
    };

    let raw = if let Some(ref attr) = source.attribute {
        target.value().attr(attr).map(|s| s.to_string())
    } else {
        Some(target.text().collect::<Vec<_>>().join("").trim().to_string())
    };

    let Some(raw) = raw else {
        return ExtractedValue::Missing;
    };

    if raw.is_empty() {
        return ExtractedValue::Missing;
    }

    match field_type {
        FieldType::Text | FieldType::DateTime => ExtractedValue::Text(raw),
        FieldType::Url => ExtractedValue::Url(raw),
        FieldType::Number => {
            let digits: String = raw.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
            digits
                .parse::<f64>()
                .map(ExtractedValue::Number)
                .unwrap_or(ExtractedValue::Text(raw))
        }
    }
}
