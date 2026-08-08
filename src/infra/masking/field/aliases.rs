// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SQL 列别名与 PromQL 派生 label 的敏感性传播。

use std::{collections::HashMap, ops::ControlFlow};

use sqlparser::{
    ast::{Expr, Select, SelectItem, Visit, Visitor, visit_expressions},
    dialect::GenericDialect,
    parser::Parser,
};

use super::{insert_algorithm, normalize_identifier};
use crate::{
    domain::{
        masking::FieldMaskingAlgorithm,
        query::{QueryLanguage, QueryRequest},
    },
    shared::{Error, Result},
};

#[derive(Default)]
struct AliasCollector {
    aliases: Vec<(String, Vec<String>)>,
}

impl Visitor for AliasCollector {
    type Break = ();

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Self::Break> {
        for item in &select.projection {
            match item {
                SelectItem::ExprWithAlias { expr, alias } => self
                    .aliases
                    .push((normalize_identifier(&alias.value), expression_fields(expr))),
                SelectItem::ExprWithAliases { expr, aliases } => {
                    let fields = expression_fields(expr);
                    self.aliases.extend(
                        aliases
                            .iter()
                            .map(|alias| (normalize_identifier(&alias.value), fields.clone())),
                    );
                }
                _ => {}
            }
        }
        ControlFlow::Continue(())
    }
}

fn expression_fields(expression: &Expr) -> Vec<String> {
    let mut fields = Vec::new();
    let _ = visit_expressions(expression, |nested| {
        match nested {
            Expr::Identifier(identifier) => {
                fields.push(normalize_identifier(&identifier.value));
            }
            Expr::CompoundIdentifier(identifiers) => {
                if let Some(identifier) = identifiers.last() {
                    fields.push(normalize_identifier(&identifier.value));
                }
            }
            _ => {}
        }
        ControlFlow::<()>::Continue(())
    });
    fields.sort();
    fields.dedup();
    fields
}

pub(super) fn propagate_derived_algorithms(
    request: &QueryRequest,
    algorithms: &mut HashMap<String, FieldMaskingAlgorithm>,
) -> Result<()> {
    let aliases = match request.language {
        QueryLanguage::Sql => {
            let statements = Parser::parse_sql(&GenericDialect, &request.statement)
                .map_err(|error| Error::invalid(format!("sqlparser: {error}")))?;
            let mut collector = AliasCollector::default();
            let _ = statements.visit(&mut collector);
            collector.aliases
        }
        QueryLanguage::Promql => {
            crate::infra::query::promql::derived_label_dependencies(&request.statement)?
        }
    };

    for _ in 0..=aliases.len() {
        let mut changed = false;
        for (alias, dependencies) in &aliases {
            let alias = normalize_identifier(alias);
            if algorithms.contains_key(&alias) {
                continue;
            }
            let inherited = dependencies
                .iter()
                .filter_map(|field| algorithms.get(&normalize_identifier(field)).cloned())
                .collect::<Vec<_>>();
            if !inherited.is_empty() {
                for algorithm in inherited {
                    insert_algorithm(algorithms, alias.clone(), algorithm);
                }
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Ok(())
}

pub(super) fn sensitive_columns(
    columns: &[String],
    algorithms: &HashMap<String, FieldMaskingAlgorithm>,
) -> HashMap<usize, FieldMaskingAlgorithm> {
    columns
        .iter()
        .enumerate()
        .filter_map(|(index, column)| {
            algorithms
                .get(&normalize_identifier(column))
                .cloned()
                .map(|algorithm| (index, algorithm))
        })
        .collect()
}
