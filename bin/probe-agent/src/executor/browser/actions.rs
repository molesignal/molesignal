// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::HashMap,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow, bail};
use headless_chrome::Tab;
use regex::Regex;
use serde_json::{Value as JsonValue, json};

use super::BrowserRuntime;
use crate::protocol::v1::{
    self as wire, AssertionResult, AssertionSeverity, ProbeOutcome, browser_step, browser_wait,
};

pub(super) struct StepExecution {
    pub outcome: ProbeOutcome,
    pub assertions: Vec<AssertionResult>,
    pub metadata: HashMap<String, String>,
}

pub(super) fn action_name(step: &wire::BrowserStep) -> &'static str {
    match step.action.as_ref() {
        Some(browser_step::Action::Navigate(_)) => "navigate",
        Some(browser_step::Action::Click(_)) => "click",
        Some(browser_step::Action::Fill(_)) => "fill",
        Some(browser_step::Action::Select(_)) => "select",
        Some(browser_step::Action::Wait(_)) => "wait",
        Some(browser_step::Action::Extract(_)) => "extract",
        Some(browser_step::Action::Assertion(_)) => "assert",
        Some(browser_step::Action::Screenshot(_)) => "screenshot",
        None => "unknown",
    }
}

pub(super) fn execute_step(
    runtime: &mut BrowserRuntime,
    step: &wire::BrowserStep,
) -> Result<StepExecution> {
    runtime.prepare_command()?;
    let action = step
        .action
        .as_ref()
        .ok_or_else(|| anyhow!("Browser step has no action"))?;
    let executed = match match action {
        browser_step::Action::Navigate(action) => navigate(runtime, action),
        browser_step::Action::Click(action) => click(runtime, action),
        browser_step::Action::Fill(action) => fill(runtime, action),
        browser_step::Action::Select(action) => select(runtime, action),
        browser_step::Action::Wait(action) => wait(runtime, action),
        browser_step::Action::Extract(action) => extract(runtime, action),
        browser_step::Action::Assertion(action) => assertion(runtime, action),
        browser_step::Action::Screenshot(action) => screenshot(runtime, action),
    } {
        Ok(executed) => executed,
        Err(error) => {
            if let Err(violation) = runtime.check_egress_violation() {
                return Err(violation)
                    .with_context(|| format!("{} step `{}` failed", action_name(step), step.name));
            }
            return Err(error)
                .with_context(|| format!("{} step `{}` failed", action_name(step), step.name));
        }
    };
    runtime.check_egress_violation()?;
    Ok(executed)
}

fn navigate(runtime: &mut BrowserRuntime, action: &wire::BrowserNavigate) -> Result<StepExecution> {
    let target = runtime.context.resolve(
        action
            .url
            .as_ref()
            .ok_or_else(|| anyhow!("Browser navigation URL is required"))?,
    )?;
    let target = runtime.validate_navigation(&target)?;
    runtime.tab.navigate_to(target.as_str())?;
    match action.wait_until.trim().to_ascii_lowercase().as_str() {
        "" | "load" => wait_for_expression(runtime, "document.readyState === 'complete'")?,
        "domcontentloaded" => wait_for_expression(
            runtime,
            "document.readyState === 'interactive' || document.readyState === 'complete'",
        )?,
        "networkidle" | "network_idle" => {
            wait_for_expression(runtime, "document.readyState === 'complete'")?;
            let quiet = Duration::from_millis(500);
            if runtime.remaining()? < quiet {
                bail!("Browser Journey timed out while waiting for network idle");
            }
            thread::sleep(quiet);
        }
        "commit" | "none" => {}
        value => bail!("unsupported Browser navigation wait condition `{value}`"),
    }
    Ok(success(HashMap::from([
        ("url".into(), runtime.context.redact(target.as_str())),
        ("wait_until".into(), action.wait_until.clone()),
        (
            "final_url".into(),
            runtime.context.redact(&runtime.tab.get_url()),
        ),
    ])))
}

fn click(runtime: &mut BrowserRuntime, action: &wire::BrowserClick) -> Result<StepExecution> {
    require_selector(&action.selector)?;
    let element = runtime.tab.wait_until_visible(&action.selector)?;
    runtime.prepare_command()?;
    element.click()?;
    runtime.prepare_command()?;
    runtime.tab.wait_until_navigated()?;
    Ok(success(selector_metadata(runtime, &action.selector)))
}

fn fill(runtime: &mut BrowserRuntime, action: &wire::BrowserFill) -> Result<StepExecution> {
    require_selector(&action.selector)?;
    let value = runtime.context.resolve(
        action
            .value
            .as_ref()
            .ok_or_else(|| anyhow!("Browser fill Value is required"))?,
    )?;
    let element = runtime.tab.wait_until_visible(&action.selector)?;
    runtime.prepare_command()?;
    element.call_js_fn(
        r#"function(value) {
            const prototype = this instanceof HTMLTextAreaElement
                ? HTMLTextAreaElement.prototype
                : HTMLInputElement.prototype;
            const setter = Object.getOwnPropertyDescriptor(prototype, 'value')?.set;
            if (setter) setter.call(this, value); else this.value = value;
            this.dispatchEvent(new Event('input', { bubbles: true }));
            this.dispatchEvent(new Event('change', { bubbles: true }));
        }"#,
        vec![json!(value)],
        false,
    )?;
    Ok(success(selector_metadata(runtime, &action.selector)))
}

fn select(runtime: &mut BrowserRuntime, action: &wire::BrowserSelect) -> Result<StepExecution> {
    require_selector(&action.selector)?;
    let value = runtime.context.resolve(
        action
            .value
            .as_ref()
            .ok_or_else(|| anyhow!("Browser select Value is required"))?,
    )?;
    let element = runtime.tab.wait_until_visible(&action.selector)?;
    runtime.prepare_command()?;
    let selected = element
        .call_js_fn(
            r#"function(value) {
                if (!(this instanceof HTMLSelectElement)) return false;
                if (![...this.options].some((option) => option.value === value)) return false;
                this.value = value;
                this.dispatchEvent(new Event('input', { bubbles: true }));
                this.dispatchEvent(new Event('change', { bubbles: true }));
                return true;
            }"#,
            vec![json!(value)],
            false,
        )?
        .value
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !selected {
        bail!("Browser select target or option is invalid");
    }
    Ok(success(selector_metadata(runtime, &action.selector)))
}

fn wait(runtime: &mut BrowserRuntime, action: &wire::BrowserWait) -> Result<StepExecution> {
    let mut metadata = HashMap::new();
    match action
        .condition
        .as_ref()
        .ok_or_else(|| anyhow!("Browser wait condition is required"))?
    {
        browser_wait::Condition::DurationMillis(duration_millis) => {
            if *duration_millis > 30_000 {
                bail!("Browser wait cannot exceed 30 seconds");
            }
            let duration = Duration::from_millis(u64::from(*duration_millis));
            if runtime.remaining()? < duration {
                bail!("Browser Journey timed out while waiting");
            }
            thread::sleep(duration);
            metadata.insert("duration_millis".into(), duration_millis.to_string());
        }
        browser_wait::Condition::Selector(selector) => {
            require_selector(selector)?;
            runtime.prepare_command()?;
            runtime.tab.wait_for_element(selector)?;
            metadata.extend(selector_metadata(runtime, selector));
        }
        browser_wait::Condition::PageExpression(expression) => {
            if expression.trim().is_empty() {
                bail!("Browser wait expression is empty");
            }
            wait_for_expression(runtime, expression)?;
            metadata.insert("expression".into(), truncate(expression, 512));
        }
    }
    Ok(success(metadata))
}

fn extract(runtime: &mut BrowserRuntime, action: &wire::BrowserExtract) -> Result<StepExecution> {
    if action.variable.trim().is_empty() {
        bail!("Browser extraction variable is empty");
    }
    runtime.prepare_command()?;
    let value = source_value(
        &runtime.tab,
        (!action.selector.trim().is_empty()).then_some(action.selector.as_str()),
        &action.source,
    )?;
    runtime
        .context
        .insert_variable(action.variable.clone(), value);
    Ok(success(HashMap::from([
        ("variable".into(), action.variable.clone()),
        ("source".into(), action.source.clone()),
        ("selector".into(), runtime.context.redact(&action.selector)),
    ])))
}

fn assertion(runtime: &mut BrowserRuntime, action: &wire::BrowserAssert) -> Result<StepExecution> {
    let assertion = action
        .assertion
        .as_ref()
        .ok_or_else(|| anyhow!("Browser Assertion is required"))?;
    runtime.prepare_command()?;
    let actual = if let Some(expression) = action
        .page_expression
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        evaluate_string(&runtime.tab, expression)?
    } else {
        source_value(&runtime.tab, action.selector.as_deref(), &assertion.source)?
    };
    let expected = assertion
        .expected
        .as_ref()
        .map(|value| runtime.context.resolve(value))
        .transpose()?
        .unwrap_or_default();
    let passed = evaluate_operator(&assertion.operator, &actual, &expected)?;
    let result = AssertionResult {
        assertion_id: assertion.id.clone(),
        severity: assertion.severity,
        passed,
        actual: truncate(&runtime.context.redact(&actual), 4096),
        message: if passed {
            String::new()
        } else {
            format!("{} assertion failed", assertion.name)
        },
    };
    let outcome = if passed {
        ProbeOutcome::Healthy
    } else if assertion.severity == AssertionSeverity::Critical as i32 {
        ProbeOutcome::Failing
    } else {
        ProbeOutcome::Degraded
    };
    Ok(StepExecution {
        outcome,
        assertions: vec![result],
        metadata: HashMap::from([
            ("assertion_id".into(), assertion.id.clone()),
            ("source".into(), assertion.source.clone()),
            ("passed".into(), passed.to_string()),
        ]),
    })
}

fn screenshot(
    runtime: &mut BrowserRuntime,
    action: &wire::BrowserScreenshot,
) -> Result<StepExecution> {
    let name = if action.name.trim().is_empty() {
        "capture"
    } else {
        action.name.trim()
    };
    let metadata = runtime.capture_screenshot(name, action.full_page)?;
    Ok(success(metadata))
}

fn source_value(tab: &Tab, selector: Option<&str>, source: &str) -> Result<String> {
    match (selector.filter(|value| !value.trim().is_empty()), source) {
        (_, "url") => Ok(tab.get_url()),
        (_, "title") => evaluate_string(tab, "document.title"),
        (None, "body" | "text" | "inner_text") => {
            evaluate_string(tab, "document.body ? document.body.innerText : ''")
        }
        (Some(selector), "text" | "inner_text") => {
            Ok(tab.wait_for_element(selector)?.get_inner_text()?)
        }
        (Some(selector), "text_content") => element_value(tab, selector, "this.textContent || ''"),
        (Some(selector), "value") => element_value(tab, selector, "this.value ?? ''"),
        (Some(selector), "html" | "outer_html") => {
            Ok(tab.wait_for_element(selector)?.get_content()?)
        }
        (Some(selector), "inner_html") => element_value(tab, selector, "this.innerHTML || ''"),
        (Some(selector), source) if source.starts_with("attribute:") => {
            let attribute = &source[10..];
            if attribute.is_empty() {
                bail!("Browser attribute source has no name");
            }
            Ok(tab
                .wait_for_element(selector)?
                .get_attribute_value(attribute)?
                .unwrap_or_default())
        }
        (None, source) if source.starts_with("expression:") => evaluate_string(tab, &source[11..]),
        _ => bail!("unsupported Browser source `{source}`"),
    }
}

fn element_value(tab: &Tab, selector: &str, expression: &str) -> Result<String> {
    let value = tab
        .wait_for_element(selector)?
        .call_js_fn(
            &format!("function() {{ return {expression}; }}"),
            vec![],
            false,
        )?
        .value
        .ok_or_else(|| anyhow!("Browser element expression returned no value"))?;
    json_value_to_string(value)
}

fn evaluate_operator(operator: &str, actual: &str, expected: &str) -> Result<bool> {
    Ok(match operator {
        "equals" => actual == expected,
        "not_equals" => actual != expected,
        "contains" => actual.contains(expected),
        "not_contains" => !actual.contains(expected),
        "matches" => Regex::new(expected)?.is_match(actual),
        "greater_than" => actual.parse::<f64>()? > expected.parse::<f64>()?,
        "less_than" => actual.parse::<f64>()? < expected.parse::<f64>()?,
        "exists" => !actual.is_empty(),
        "json_schema" => {
            serde_json::from_str::<JsonValue>(actual).is_ok()
                && serde_json::from_str::<JsonValue>(expected).is_ok()
        }
        value => bail!("unsupported Browser Assertion operator `{value}`"),
    })
}

fn wait_for_expression(runtime: &BrowserRuntime, expression: &str) -> Result<()> {
    let deadline = Instant::now() + runtime.remaining()?;
    loop {
        runtime.prepare_command()?;
        if evaluate_bool(&runtime.tab, expression)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("Browser expression did not become true before timeout");
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn evaluate_bool(tab: &Tab, expression: &str) -> Result<bool> {
    let value = tab
        .evaluate(expression, true)?
        .value
        .ok_or_else(|| anyhow!("Browser expression returned no value"))?;
    value
        .as_bool()
        .ok_or_else(|| anyhow!("Browser wait expression must return a boolean"))
}

pub(super) fn evaluate_string(tab: &Tab, expression: &str) -> Result<String> {
    let value = tab
        .evaluate(expression, true)?
        .value
        .ok_or_else(|| anyhow!("Browser expression returned no value"))?;
    json_value_to_string(value)
}

fn json_value_to_string(value: JsonValue) -> Result<String> {
    match value {
        JsonValue::String(value) => Ok(value),
        JsonValue::Null => Ok(String::new()),
        value => serde_json::to_string(&value).map_err(Into::into),
    }
}

fn success(metadata: HashMap<String, String>) -> StepExecution {
    StepExecution {
        outcome: ProbeOutcome::Healthy,
        assertions: Vec::new(),
        metadata,
    }
}

fn selector_metadata(runtime: &BrowserRuntime, selector: &str) -> HashMap<String, String> {
    HashMap::from([("selector".into(), runtime.context.redact(selector))])
}

fn require_selector(selector: &str) -> Result<()> {
    if selector.trim().is_empty() {
        bail!("Browser selector is empty");
    }
    Ok(())
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}
