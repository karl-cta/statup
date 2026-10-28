//! Every accent and ambiance an administrator can pick keeps the page
//! readable: the values are read from `static/css/tokens.css` and checked
//! against the WCAG AA ratios, in both themes, so a colour added to the list
//! cannot ship unreadable.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use statup::models::{Accent, Ambiance};

const TEXT: f64 = 4.5;
const CONTROL: f64 = 3.0;

type Block = HashMap<String, String>;

/// Each rule of the file, by each of its selectors, as its custom properties.
fn blocks() -> HashMap<String, Block> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("static/css/tokens.css");
    let css = without_comments(&fs::read_to_string(path).unwrap());
    let mut blocks = HashMap::new();
    let mut rest = css.as_str();
    while let Some(open) = rest.find('{') {
        let selectors = rest[..open].to_string();
        let close = open + rest[open..].find('}').unwrap();
        let body = &rest[open + 1..close];
        assert!(
            !body.contains('{'),
            "tokens.css gained a nested block: teach this reader to walk it"
        );
        let properties: Block = body
            .split(';')
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                let name = name.trim();
                name.starts_with("--")
                    .then(|| (name.to_string(), value.trim().to_string()))
            })
            .collect();
        for selector in selectors.split(',') {
            blocks.insert(selector.trim().to_string(), properties.clone());
        }
        rest = &rest[close + 1..];
    }
    blocks
}

fn without_comments(css: &str) -> String {
    let mut kept = String::new();
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        kept.push_str(&rest[..start]);
        let end = rest[start..].find("*/").unwrap();
        rest = &rest[start + end + 2..];
    }
    kept.push_str(rest);
    kept
}

/// The properties `<html>` ends with, in the cascade order of the file.
fn resolve(
    blocks: &HashMap<String, Block>,
    dark: bool,
    ambiance: Ambiance,
    accent: Accent,
) -> Block {
    let layer = |prefix: &str| {
        [
            ambiance
                .attribute()
                .map(|id| format!("{prefix}[data-ambiance=\"{id}\"]")),
            accent
                .attribute()
                .map(|id| format!("{prefix}[data-accent=\"{id}\"]")),
        ]
    };
    let mut selectors = vec![Some("@theme".to_string()), Some(":root".to_string())];
    selectors.extend(layer(""));
    if dark {
        selectors.push(Some(".dark".to_string()));
        selectors.extend(layer(".dark"));
    }
    let mut resolved = Block::new();
    for selector in selectors.into_iter().flatten() {
        if let Some(block) = blocks.get(&selector) {
            resolved.extend(block.clone());
        }
    }
    resolved
}

fn colour(properties: &Block, name: &str) -> String {
    let lookup = |key: &str| {
        properties
            .get(key)
            .unwrap_or_else(|| panic!("{key} is not defined"))
            .clone()
    };
    let mut value = lookup(name);
    while let Some(inner) = value.strip_prefix("var(").and_then(|v| v.strip_suffix(')')) {
        value = lookup(inner);
    }
    value
}

fn luminance(hex: &str) -> f64 {
    let channel = |at: usize| {
        let value = f64::from(u8::from_str_radix(&hex[at..at + 2], 16).unwrap()) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5)
}

fn ratio(properties: &Block, front: &str, back: &str) -> f64 {
    let (a, b) = (colour(properties, front), colour(properties, back));
    assert!(
        a.len() == 7 && b.len() == 7 && a.starts_with('#') && b.starts_with('#'),
        "{front} ({a}) and {back} ({b}) must resolve to #RRGGBB"
    );
    let (light, dark) = {
        let (la, lb) = (luminance(&a), luminance(&b));
        (la.max(lb), la.min(lb))
    };
    (light + 0.05) / (dark + 0.05)
}

fn check(
    failures: &mut Vec<String>,
    properties: &Block,
    front: &str,
    back: &str,
    floor: f64,
    case: &str,
) {
    let measured = ratio(properties, front, back);
    if measured < floor {
        failures.push(format!(
            "{case}: {front} on {back} is {measured:.2}, under {floor}"
        ));
    }
}

#[test]
fn every_choice_has_its_rules() {
    let blocks = blocks();
    for accent in Accent::ALL.into_iter().filter_map(Accent::attribute) {
        for prefix in ["", ".dark"] {
            let selector = format!("{prefix}[data-accent=\"{accent}\"]");
            assert!(blocks.contains_key(&selector), "missing {selector}");
        }
    }
    for ambiance in Ambiance::ALL.into_iter().filter_map(Ambiance::attribute) {
        let selector = format!("[data-ambiance=\"{ambiance}\"]");
        assert!(blocks.contains_key(&selector), "missing {selector}");
    }
}

#[test]
fn every_accent_reads_in_every_ambiance_and_theme() {
    let blocks = blocks();
    let mut failures = Vec::new();
    for dark in [false, true] {
        for ambiance in Ambiance::ALL {
            for accent in Accent::ALL {
                let case = format!("{} {} dark={dark}", accent.as_str(), ambiance.as_str());
                let properties = resolve(&blocks, dark, ambiance, accent);
                for state in ["--primary", "--primary-hover", "--primary-active"] {
                    check(
                        &mut failures,
                        &properties,
                        "--on-primary",
                        state,
                        TEXT,
                        &case,
                    );
                }
                for ground in ["--ground", "--card"] {
                    check(
                        &mut failures,
                        &properties,
                        "--primary",
                        ground,
                        CONTROL,
                        &case,
                    );
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_ambiance_keeps_its_text_and_states_readable() {
    let blocks = blocks();
    let mut failures = Vec::new();
    for dark in [false, true] {
        for ambiance in Ambiance::ALL {
            let case = format!("{} dark={dark}", ambiance.as_str());
            let properties = resolve(&blocks, dark, ambiance, Accent::default());
            for ground in ["--ground", "--card", "--raised", "--menu"] {
                for text in ["--ink", "--muted"] {
                    check(&mut failures, &properties, text, ground, TEXT, &case);
                }
            }
            for state in [
                "--sem-ok-text",
                "--sem-minor-text",
                "--sem-crit-text",
                "--sem-info-text",
            ] {
                for ground in ["--ground", "--card"] {
                    check(&mut failures, &properties, state, ground, TEXT, &case);
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
