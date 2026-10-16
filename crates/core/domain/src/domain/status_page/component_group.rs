// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Components can be grouped the way folders hold files. A group holds
//! components and has no status of its own: the page shows it with the worst
//! status among the components in it, and the components under it. A group
//! holds components, not other groups.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{ComponentLifecycle, ComponentStatus, StatusPageComponent};
use crate::shared::ids::Id;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    /// Something customers rely on, with a status of its own.
    #[default]
    Component,
    /// A group of components.
    Group,
}

impl ComponentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Component => "component",
            Self::Group => "group",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "component" => Some(Self::Component),
            "group" => Some(Self::Group),
            _ => None,
        }
    }
}

impl StatusPageComponent {
    pub fn is_group(&self) -> bool {
        self.kind == ComponentKind::Group
    }

    /// Whether the component is in the group named.
    pub fn is_in(&self, group_id: &Id) -> bool {
        self.group_id.as_ref() == Some(group_id)
    }
}

/// The active components of the group named, among the components of its page.
pub fn group_members<'a>(
    components: &'a [StatusPageComponent],
    group_id: &'a Id,
) -> impl Iterator<Item = &'a StatusPageComponent> + 'a {
    components.iter().filter(move |component| {
        component.is_in(group_id) && component.lifecycle == ComponentLifecycle::Active
    })
}

/// The groups of the components a reader is shown: each with the worst status
/// among its components, and only while one of them is shown. A component
/// whose group is not shown stands on its own.
pub fn settle_component_groups(components: &mut Vec<StatusPageComponent>) {
    let mut worst: HashMap<String, ComponentStatus> = HashMap::new();

    for component in components.iter().filter(|component| !component.is_group()) {
        if let Some(group_id) = &component.group_id {
            let status = worst
                .entry(group_id.0.clone())
                .or_insert(ComponentStatus::Operational);

            *status = ComponentStatus::worst(*status, component.status);
        }
    }

    components
        .retain(|component| !component.is_group() || worst.contains_key(component.id.as_str()));

    let shown: HashSet<String> = components
        .iter()
        .filter(|component| component.is_group())
        .map(|component| component.id.0.clone())
        .collect();

    for component in components.iter_mut() {
        if component.is_group() {
            if let Some(status) = worst.get(component.id.as_str()) {
                component.status = *status;
            }
        } else if component
            .group_id
            .as_ref()
            .is_some_and(|group_id| !shown.contains(group_id.as_str()))
        {
            component.group_id = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::status_page::ComponentVisibility, shared::time::TimestampMicros};

    fn component(
        id: &str,
        kind: ComponentKind,
        group: Option<&str>,
        status: ComponentStatus,
    ) -> StatusPageComponent {
        StatusPageComponent {
            id: Id(id.into()),
            org_id: Id("org".into()),
            status_page_id: Id("page".into()),
            name: id.into(),
            description: String::new(),
            status,
            visibility: ComponentVisibility::Enabled,
            lifecycle: ComponentLifecycle::Active,
            position: 0,
            archived_at: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
            business_service: None,
            kind,
            group_id: group.map(|group| Id(group.into())),
        }
    }

    #[test]
    fn a_group_shows_the_worst_status_of_its_components_and_an_empty_one_is_not_shown() {
        let mut components = vec![
            component(
                "ai",
                ComponentKind::Group,
                None,
                ComponentStatus::Operational,
            ),
            component(
                "chat",
                ComponentKind::Component,
                Some("ai"),
                ComponentStatus::Operational,
            ),
            component(
                "sandbox",
                ComponentKind::Component,
                Some("ai"),
                ComponentStatus::PartialOutage,
            ),
            component(
                "empty",
                ComponentKind::Group,
                None,
                ComponentStatus::Operational,
            ),
            component(
                "api",
                ComponentKind::Component,
                None,
                ComponentStatus::Operational,
            ),
        ];

        settle_component_groups(&mut components);

        let ids: Vec<&str> = components
            .iter()
            .map(|component| component.id.as_str())
            .collect();
        assert_eq!(ids, ["ai", "chat", "sandbox", "api"]);
        assert_eq!(components[0].status, ComponentStatus::PartialOutage);
    }

    #[test]
    fn a_component_whose_group_is_not_shown_stands_on_its_own() {
        let mut components = vec![component(
            "chat",
            ComponentKind::Component,
            Some("hidden"),
            ComponentStatus::Operational,
        )];

        settle_component_groups(&mut components);
        assert_eq!(components[0].group_id, None);
    }
}
