// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 顶层 `WHERE` 的条件树：查询侧用索引剔除不可能有结果的文件。
//!
//! 叶子是 `AND`、`OR` 与括号之外的每一个表达式（原子条件），按出现次序编号。剪枝的每一步只证明
//! 自己看得懂的原子条件在某个文件里没有一行成立；在一个文件里，被证明不成立的原子当作假、其余当作
//! 可能成立，整棵树仍可能成立，这个文件就得留下：`AND` 要每一支都可能成立，`OR` 有一支可能成立即
//! 可。不同的原子可以由不同的索引证明：`level = 'error' OR status >= 500` 的两支一支靠整值索引、
//! 一支靠文件统计，两支都被证明不成立的文件才不读。
//!
//! 只看顶层 SELECT，且 `FROM` 只有一张表、不连接（见 [`reads_one_table`]）：子查询与 CTE 的条件
//! 作用于别的表。别的语句、解析失败时树是空的，不剔除任何文件。

use sqlparser::{
    ast::{BinaryOperator, Expr, SetExpr, Statement},
    dialect::GenericDialect,
    parser::Parser,
};

use crate::reads_one_table;

/// 最多编这么多号：每个文件用一个 [`Refuted`] 记下哪些原子被证明不成立。更多的原子当作可能成立。
pub const MAX_ATOMS: usize = 64;

/// 被证明不成立的原子，第 `i` 位是第 `i` 个原子。
pub type Refuted = u64;

/// 第 `atom` 个原子的那一位。
pub fn atom_bit(atom: usize) -> Refuted {
    1 << atom
}

/// 一条语句顶层 `WHERE` 的条件树。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WhereTree {
    root: Option<Node>,
    atoms: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Atom(usize),
    All(Vec<Node>),
    Any(Vec<Node>),
}

impl WhereTree {
    /// `sql` 的条件树；没有能用来剔除文件的条件时是空的。
    pub fn parse(sql: &str) -> Self {
        let Ok(statements) = Parser::parse_sql(&GenericDialect, sql) else {
            return Self::default();
        };

        let [Statement::Query(query)] = statements.as_slice() else {
            return Self::default();
        };

        let SetExpr::Select(select) = query.body.as_ref() else {
            return Self::default();
        };

        let Some(selection) = select
            .selection
            .as_ref()
            .filter(|_| reads_one_table(select))
        else {
            return Self::default();
        };

        let mut atoms = Vec::new();
        let root = node(selection, &mut atoms);

        Self { root, atoms }
    }

    /// 原子条件与它们的编号。
    pub fn atoms(&self) -> impl Iterator<Item = (usize, &Expr)> {
        self.atoms.iter().enumerate()
    }

    /// 每一行都满足的原子：顶层 `AND` 链上的。
    pub fn conjuncts(&self) -> Vec<usize> {
        match &self.root {
            Some(Node::Atom(atom)) => vec![*atom],
            Some(Node::All(children)) => children
                .iter()
                .filter_map(|child| match child {
                    Node::Atom(atom) => Some(*atom),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `refuted` 里的原子都不成立时，整棵树仍可能成立。空树总是可能成立。
    pub fn may_hold(&self, refuted: Refuted) -> bool {
        self.root.as_ref().is_none_or(|root| root.may_hold(refuted))
    }

    /// 满足整棵树的行数的上界，`bound` 给出每个原子的上界：`AND` 取各支上界里最小的，不计没有
    /// 上界的支；`OR` 是各支上界之和，有一支没有上界它就没有。
    pub fn bound(&self, bound: &dyn Fn(usize) -> Option<u64>) -> Option<u64> {
        self.root.as_ref()?.bound(bound)
    }
}

impl Node {
    fn may_hold(&self, refuted: Refuted) -> bool {
        match self {
            Self::Atom(atom) => refuted & atom_bit(*atom) == 0,
            Self::All(children) => children.iter().all(|child| child.may_hold(refuted)),
            Self::Any(children) => children.iter().any(|child| child.may_hold(refuted)),
        }
    }

    fn bound(&self, bound: &dyn Fn(usize) -> Option<u64>) -> Option<u64> {
        match self {
            Self::Atom(atom) => bound(*atom),
            Self::All(children) => children.iter().filter_map(|child| child.bound(bound)).min(),
            Self::Any(children) => children
                .iter()
                .map(|child| child.bound(bound))
                .try_fold(0_u64, |sum, child| Some(sum.saturating_add(child?))),
        }
    }
}

/// `expr` 的子树；整支都可能成立（没有能编号的原子）时为 `None`。
fn node(expr: &Expr, atoms: &mut Vec<Expr>) -> Option<Node> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            // 证明不了的一支对 `AND` 没有约束，略去即可。
            let mut children = Vec::new();

            for side in [left, right] {
                match node(side, atoms) {
                    Some(Node::All(inner)) => children.extend(inner),
                    Some(child) => children.push(child),
                    None => {}
                }
            }

            match children.len() {
                0 => None,
                1 => children.pop(),
                _ => Some(Node::All(children)),
            }
        }
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Or,
            right,
        } => {
            // 有一支证明不了，整个 `OR` 就都可能成立。
            let mut children = Vec::new();

            for side in [left, right] {
                match node(side, atoms)? {
                    Node::Any(inner) => children.extend(inner),
                    child => children.push(child),
                }
            }

            Some(Node::Any(children))
        }
        Expr::Nested(inner) => node(inner, atoms),
        other if atoms.len() < MAX_ATOMS => {
            atoms.push(other.clone());
            Some(Node::Atom(atoms.len() - 1))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 文本里含 `needle` 的原子的那一位。
    fn bit(tree: &WhereTree, needle: &str) -> Refuted {
        let (atom, _) = tree
            .atoms()
            .find(|(_, expr)| expr.to_string().contains(needle))
            .unwrap_or_else(|| panic!("no atom with {needle}"));

        atom_bit(atom)
    }

    #[test]
    fn an_and_falls_with_any_branch_and_an_or_only_with_every_branch() {
        let tree = WhereTree::parse(
            "SELECT * FROM logs WHERE service = 'api' AND (level = 'error' OR status >= 500)",
        );

        assert_eq!(tree.atoms().count(), 3);
        assert_eq!(tree.conjuncts(), [0]);
        assert!(tree.may_hold(0));
        assert!(!tree.may_hold(bit(&tree, "service")));
        assert!(tree.may_hold(bit(&tree, "level")));
        assert!(tree.may_hold(bit(&tree, "status")));
        assert!(!tree.may_hold(bit(&tree, "level") | bit(&tree, "status")));
    }

    /// 证明不了的原子也编号，只是没有哪一步能证明它不成立：它所在的 `OR` 永远可能成立。
    #[test]
    fn a_branch_nothing_refutes_keeps_its_or_standing() {
        let tree = WhereTree::parse(
            "SELECT * FROM logs WHERE level = 'error' OR lower(message) = 'x' OR status > 1",
        );

        assert!(tree.may_hold(bit(&tree, "level") | bit(&tree, "status")));
    }

    #[test]
    fn joins_subqueries_and_other_statements_give_an_empty_tree() {
        for sql in [
            "SELECT * FROM a JOIN b ON a.id = b.id WHERE a.x = 'y'",
            "SELECT * FROM (SELECT * FROM logs) AS t WHERE x = 'y'",
            "SELECT * FROM logs",
            "SELECT * FROM a WHERE x = 'y' UNION SELECT * FROM b WHERE x = 'z'",
            "not sql",
        ] {
            let tree = WhereTree::parse(sql);

            assert_eq!(tree.atoms().count(), 0, "{sql}");
            assert!(tree.may_hold(Refuted::MAX), "{sql}");
        }
    }

    #[test]
    fn bounds_take_the_least_of_an_and_and_the_sum_of_an_or() {
        let tree = WhereTree::parse(
            "SELECT * FROM logs WHERE a = 'x' AND (b = 'y' OR b = 'z') AND message LIKE '%m%'",
        );

        let counts = |atom: usize| match tree.atoms().nth(atom).unwrap().1.to_string() {
            text if text.starts_with("a =") => Some(10),
            text if text.contains("'y'") => Some(2),
            text if text.contains("'z'") => Some(3),
            _ => None,
        };

        assert_eq!(tree.bound(&counts), Some(5));
        assert_eq!(tree.bound(&|_| None), None);

        let either = WhereTree::parse("SELECT * FROM logs WHERE a = 'x' OR message LIKE '%m%'");
        assert_eq!(either.bound(&|atom| (atom == 0).then_some(4)), None);
    }

    #[test]
    fn atoms_past_the_limit_count_as_holding() {
        let conditions = (0..MAX_ATOMS + 2)
            .map(|index| format!("f{index} = 'v'"))
            .collect::<Vec<_>>();

        let all = WhereTree::parse(&format!(
            "SELECT * FROM logs WHERE {}",
            conditions.join(" AND ")
        ));
        assert_eq!(all.atoms().count(), MAX_ATOMS);
        assert!(!all.may_hold(atom_bit(0)));

        let any = WhereTree::parse(&format!(
            "SELECT * FROM logs WHERE {}",
            conditions.join(" OR ")
        ));
        assert!(any.may_hold(Refuted::MAX));
    }
}
