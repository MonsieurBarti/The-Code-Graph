use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Eq, Neq, Lt, Lte, Gt, Gte, Like, In,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    pub field: String,
    pub op: Operator,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortClause {
    pub field: String,
    pub ascending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPlan {
    pub table: String,
    pub filters: Vec<Filter>,
    pub sort: Vec<SortClause>,
    pub limit: Option<usize>,
    pub offset: usize,
    pub select: Vec<String>,
}

impl QueryPlan {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            filters: Vec::new(),
            sort: Vec::new(),
            limit: None,
            offset: 0,
            select: Vec::new(),
        }
    }

    pub fn filter(mut self, field: &str, op: Operator, value: impl Into<serde_json::Value>) -> Self {
        self.filters.push(Filter { field: field.to_string(), op, value: value.into() });
        self
    }

    pub fn sort_by(mut self, field: &str, ascending: bool) -> Self {
        self.sort.push(SortClause { field: field.to_string(), ascending });
        self
    }

    pub fn limit(mut self, n: usize) -> Self { self.limit = Some(n); self }
    pub fn offset(mut self, n: usize) -> Self { self.offset = n; self }
    pub fn select(mut self, fields: &[&str]) -> Self {
        self.select = fields.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn to_sql(&self) -> String {
        let select = if self.select.is_empty() { "*".to_string() } else { self.select.join(", ") };
        let mut sql = format!("SELECT {} FROM {}", select, self.table);
        if !self.filters.is_empty() {
            let clauses: Vec<String> = self.filters.iter().map(|f| format!("{} = ?", f.field)).collect();
            sql.push_str(&format!(" WHERE {}", clauses.join(" AND ")));
        }
        if !self.sort.is_empty() {
            let order: Vec<String> = self.sort.iter().map(|s| format!("{} {}", s.field, if s.ascending { "ASC" } else { "DESC" })).collect();
            sql.push_str(&format!(" ORDER BY {}", order.join(", ")));
        }
        if let Some(lim) = self.limit { sql.push_str(&format!(" LIMIT {}", lim)); }
        if self.offset > 0 { sql.push_str(&format!(" OFFSET {}", self.offset)); }
        sql
    }
}
