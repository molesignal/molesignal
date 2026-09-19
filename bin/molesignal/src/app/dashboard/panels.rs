// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::Value;

use super::DashboardService;
use crate::{
    domain::dashboard::Dashboard,
    shared::{Error, Result, ids::Id},
};

impl DashboardService {
    pub async fn add_panel(
        &self,
        mut dashboard: Dashboard,
        actor: Id,
        panel: Value,
        container_id: Option<&str>,
        position: Option<usize>,
        expected_version: Option<u32>,
    ) -> Result<Dashboard> {
        ensure_version(&dashboard, expected_version)?;
        let panel_id = panel_id(&panel)?.to_string();
        if find_element(&dashboard.model, &panel_id).is_some() {
            return Err(Error::conflict("dashboard element id already exists"));
        }
        let elements = container_elements_mut(&mut dashboard.model, container_id)?;
        let position = position.unwrap_or(elements.len()).min(elements.len());
        elements.insert(position, panel);
        let folder_id = dashboard.folder_id.clone();
        let model = dashboard.model.clone();
        self.update_model(dashboard, folder_id, actor, model).await
    }

    pub async fn update_panel(
        &self,
        mut dashboard: Dashboard,
        actor: Id,
        panel_id_from_path: &str,
        panel: Value,
        expected_version: Option<u32>,
    ) -> Result<Dashboard> {
        ensure_version(&dashboard, expected_version)?;
        if panel_id(&panel)? != panel_id_from_path {
            return Err(Error::invalid("panel body id must match the path panel id"));
        }
        if !replace_panel(&mut dashboard.model, panel_id_from_path, panel) {
            return Err(Error::not_found("dashboard panel not found"));
        }
        let folder_id = dashboard.folder_id.clone();
        let model = dashboard.model.clone();
        self.update_model(dashboard, folder_id, actor, model).await
    }

    pub async fn delete_panel(
        &self,
        mut dashboard: Dashboard,
        actor: Id,
        panel_id: &str,
        expected_version: Option<u32>,
    ) -> Result<Dashboard> {
        ensure_version(&dashboard, expected_version)?;
        if remove_panel(&mut dashboard.model, panel_id).is_none() {
            return Err(Error::not_found("dashboard panel not found"));
        }
        let folder_id = dashboard.folder_id.clone();
        let model = dashboard.model.clone();
        self.update_model(dashboard, folder_id, actor, model).await
    }

    pub async fn move_panel(
        &self,
        mut dashboard: Dashboard,
        actor: Id,
        panel_id: &str,
        container_id: Option<&str>,
        position: usize,
        expected_version: Option<u32>,
    ) -> Result<Dashboard> {
        ensure_version(&dashboard, expected_version)?;
        let panel = remove_panel(&mut dashboard.model, panel_id)
            .ok_or_else(|| Error::not_found("dashboard panel not found"))?;
        let elements = container_elements_mut(&mut dashboard.model, container_id)?;
        elements.insert(position.min(elements.len()), panel);
        let folder_id = dashboard.folder_id.clone();
        let model = dashboard.model.clone();
        self.update_model(dashboard, folder_id, actor, model).await
    }
}

fn ensure_version(dashboard: &Dashboard, expected: Option<u32>) -> Result<()> {
    if expected.is_some_and(|expected| expected != dashboard.version) {
        Err(Error::conflict(format!(
            "dashboard version changed; current version is {}",
            dashboard.version
        )))
    } else {
        Ok(())
    }
}

fn panel_id(panel: &Value) -> Result<&str> {
    let object = panel
        .as_object()
        .ok_or_else(|| Error::invalid("panel must be an object"))?;
    if object.get("kind").and_then(Value::as_str) != Some("panel") {
        return Err(Error::invalid("dashboard panel kind must be `panel`"));
    }
    object
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Error::invalid("dashboard panel id must not be empty"))
}

fn root_elements_mut(model: &mut Value) -> Result<&mut Vec<Value>> {
    model
        .get_mut("elements")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| Error::invalid("dashboard model elements must be an array"))
}

fn container_elements_mut<'a>(
    model: &'a mut Value,
    container_id: Option<&str>,
) -> Result<&'a mut Vec<Value>> {
    match container_id {
        None => root_elements_mut(model),
        Some(id) => {
            let pointer = model
                .get("elements")
                .and_then(Value::as_array)
                .and_then(|elements| container_pointer(elements, id, "/elements"))
                .ok_or_else(|| Error::not_found("dashboard panel container not found"))?;
            model
                .pointer_mut(&pointer)
                .and_then(Value::as_array_mut)
                .ok_or_else(|| Error::not_found("dashboard panel container not found"))
        }
    }
}

fn container_pointer(elements: &[Value], id: &str, prefix: &str) -> Option<String> {
    for (element_index, element) in elements.iter().enumerate() {
        let element_prefix = format!("{prefix}/{element_index}");
        if element.get("id").and_then(Value::as_str) == Some(id)
            && matches!(
                element.get("kind").and_then(Value::as_str),
                Some("group" | "row")
            )
        {
            return Some(format!("{element_prefix}/elements"));
        }
        if let Some(children) = element.get("elements").and_then(Value::as_array)
            && let Some(found) =
                container_pointer(children, id, &format!("{element_prefix}/elements"))
        {
            return Some(found);
        }
        if let Some(tabs) = element.get("tabs").and_then(Value::as_array) {
            for (tab_index, tab) in tabs.iter().enumerate() {
                let tab_prefix = format!("{element_prefix}/tabs/{tab_index}");
                if tab.get("id").and_then(Value::as_str) == Some(id) {
                    return Some(format!("{tab_prefix}/elements"));
                }
                if let Some(children) = tab.get("elements").and_then(Value::as_array)
                    && let Some(found) =
                        container_pointer(children, id, &format!("{tab_prefix}/elements"))
                {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn find_element<'a>(model: &'a Value, id: &str) -> Option<&'a Value> {
    find_in_elements(model.get("elements")?.as_array()?, id)
}

fn find_in_elements<'a>(elements: &'a [Value], id: &str) -> Option<&'a Value> {
    for element in elements {
        if element.get("id").and_then(Value::as_str) == Some(id) {
            return Some(element);
        }
        if let Some(found) = element
            .get("elements")
            .and_then(Value::as_array)
            .and_then(|children| find_in_elements(children, id))
        {
            return Some(found);
        }
        if let Some(tabs) = element.get("tabs").and_then(Value::as_array) {
            for tab in tabs {
                if tab.get("id").and_then(Value::as_str) == Some(id) {
                    return Some(tab);
                }
                if let Some(found) = tab
                    .get("elements")
                    .and_then(Value::as_array)
                    .and_then(|children| find_in_elements(children, id))
                {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn replace_panel(model: &mut Value, id: &str, replacement: Value) -> bool {
    root_elements_mut(model).is_ok_and(|elements| replace_in_elements(elements, id, replacement))
}

fn replace_in_elements(elements: &mut [Value], id: &str, replacement: Value) -> bool {
    for element in elements {
        if element.get("kind").and_then(Value::as_str) == Some("panel")
            && element.get("id").and_then(Value::as_str) == Some(id)
        {
            *element = replacement;
            return true;
        }
        if let Some(children) = element.get_mut("elements").and_then(Value::as_array_mut)
            && replace_in_elements(children, id, replacement.clone())
        {
            return true;
        }
        if let Some(tabs) = element.get_mut("tabs").and_then(Value::as_array_mut) {
            for tab in tabs {
                if let Some(children) = tab.get_mut("elements").and_then(Value::as_array_mut)
                    && replace_in_elements(children, id, replacement.clone())
                {
                    return true;
                }
            }
        }
    }
    false
}

fn remove_panel(model: &mut Value, id: &str) -> Option<Value> {
    remove_from_elements(root_elements_mut(model).ok()?, id)
}

fn remove_from_elements(elements: &mut Vec<Value>, id: &str) -> Option<Value> {
    if let Some(index) = elements.iter().position(|element| {
        element.get("kind").and_then(Value::as_str) == Some("panel")
            && element.get("id").and_then(Value::as_str) == Some(id)
    }) {
        return Some(elements.remove(index));
    }
    for element in elements {
        if let Some(children) = element.get_mut("elements").and_then(Value::as_array_mut)
            && let Some(panel) = remove_from_elements(children, id)
        {
            return Some(panel);
        }
        if let Some(tabs) = element.get_mut("tabs").and_then(Value::as_array_mut) {
            for tab in tabs {
                if let Some(children) = tab.get_mut("elements").and_then(Value::as_array_mut)
                    && let Some(panel) = remove_from_elements(children, id)
                {
                    return Some(panel);
                }
            }
        }
    }
    None
}
