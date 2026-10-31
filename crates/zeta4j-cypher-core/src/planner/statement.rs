// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Statement Planning
//!
//! Statement 실행 계획 생성 함수들

use super::QueryPlanner;
use crate::ast::*;
use zeta4j_graph::error::Result;

impl QueryPlanner {
    /// Statement의 실행 계획 생성
    pub(crate) fn plan_statement(&self, statement: &Statement) -> Result<PlanNode> {
        match statement {
            Statement::Reading(reading) => self.plan_reading(reading),
            Statement::Updating(updating) => self.plan_updating(updating),
            Statement::With(with_clause) => self.plan_with(with_clause),
            Statement::Unwind(unwind) => self.plan_unwind(unwind),
            Statement::Return(return_clause) => self.plan_return(return_clause),
            Statement::Finish => Ok(PlanNode::new(
                PlanNodeType::EmptyResult,
                "FINISH".to_string(),
            )),
            Statement::Let(let_clause) => Ok(PlanNode::new(
                PlanNodeType::Projection,
                format!("LET {} = ...", let_clause.variable),
            )),
            Statement::Use(use_clause) => {
                let db_desc = match &use_clause.database {
                    DatabaseTarget::Named(name) => name.clone(),
                    DatabaseTarget::Dynamic(_) => "graph.byName(...)".to_string(),
                };
                Ok(PlanNode::new(
                    PlanNodeType::Admin,
                    format!("USE {}", db_desc),
                ))
            }
            Statement::Call(call) => self.plan_call(call),
            Statement::Procedure(proc_call) => self.plan_procedure(proc_call),
            Statement::Index(cmd) => self.plan_index(cmd),
            Statement::Schema(cmd) => self.plan_schema(cmd),
            Statement::Constraint(cmd) => self.plan_constraint(cmd),
            Statement::Admin(cmd) => self.plan_admin(cmd),
        }
    }

    /// 스키마 명령 실행 계획
    fn plan_schema(&self, cmd: &SchemaCommand) -> Result<PlanNode> {
        match cmd {
            SchemaCommand::CreateNodeLabel { label, properties } => {
                let props_desc = if properties.is_empty() {
                    String::new()
                } else {
                    format!(
                        "({})",
                        properties
                            .iter()
                            .map(|p| format!("{} {}", p.name, p.property_type))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                Ok(PlanNode::new(
                    PlanNodeType::Create,
                    format!("NODE LABEL :{}{}", label, props_desc),
                ))
            }
            SchemaCommand::CreateRelationshipType {
                rel_type,
                properties,
                from_labels,
                to_labels,
            } => {
                let mut desc = format!("RELATIONSHIP TYPE :{}", rel_type);
                if !properties.is_empty() {
                    desc.push_str(&format!(
                        "({})",
                        properties
                            .iter()
                            .map(|p| format!("{} {}", p.name, p.property_type))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if let Some(from) = from_labels {
                    desc.push_str(&format!(" FROM :{}", from.join(", ")));
                }
                if let Some(to) = to_labels {
                    desc.push_str(&format!(" TO :{}", to.join(", ")));
                }
                Ok(PlanNode::new(PlanNodeType::Create, desc))
            }
            SchemaCommand::DropNodeLabel { label } => Ok(PlanNode::new(
                PlanNodeType::Delete,
                format!("NODE LABEL :{}", label),
            )),
            SchemaCommand::DropRelationshipType { rel_type } => Ok(PlanNode::new(
                PlanNodeType::Delete,
                format!("RELATIONSHIP TYPE :{}", rel_type),
            )),
            SchemaCommand::ShowNodeLabels => Ok(PlanNode::new(
                PlanNodeType::ProduceResults,
                "SHOW NODE LABELS".to_string(),
            )),
            SchemaCommand::ShowRelationshipTypes => Ok(PlanNode::new(
                PlanNodeType::ProduceResults,
                "SHOW RELATIONSHIP TYPES".to_string(),
            )),
            SchemaCommand::ShowSchemaFor { is_node: _, name } => Ok(PlanNode::new(
                PlanNodeType::ProduceResults,
                format!("SHOW SCHEMA FOR :{}", name),
            )),
        }
    }

    /// 인덱스 명령 실행 계획
    fn plan_index(&self, cmd: &IndexCommand) -> Result<PlanNode> {
        match cmd {
            IndexCommand::Create {
                name,
                label,
                properties,
            } => {
                let desc = format!(
                    "{}ON :{}({})",
                    name.as_ref().map_or(String::new(), |n| format!("{} ", n)),
                    label,
                    properties.join(", ")
                );
                Ok(PlanNode::new(
                    PlanNodeType::Create,
                    format!("INDEX {}", desc),
                ))
            }
            IndexCommand::Drop {
                name,
                label,
                properties,
            } => {
                let desc = if let Some(n) = name {
                    n.clone()
                } else if let Some(l) = label {
                    format!("ON :{}({})", l, properties.join(", "))
                } else {
                    String::new()
                };
                Ok(PlanNode::new(
                    PlanNodeType::Delete,
                    format!("INDEX {}", desc),
                ))
            }
            IndexCommand::Show => Ok(PlanNode::new(PlanNodeType::ProduceResults, "SHOW INDEXES")),
            IndexCommand::CreateSpatial { label, property } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!("SPATIAL INDEX ON :{}({})", label, property),
            )),
            IndexCommand::DropSpatial { label, property } => Ok(PlanNode::new(
                PlanNodeType::Delete,
                format!("SPATIAL INDEX ON :{}({})", label, property),
            )),
            IndexCommand::ShowSpatial => Ok(PlanNode::new(
                PlanNodeType::ProduceResults,
                "SHOW SPATIAL INDEXES",
            )),
            IndexCommand::CreateFulltext {
                name,
                label,
                properties,
                analyzer,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "FULLTEXT INDEX {name} FOR :{label}({}) ANALYZER {analyzer}",
                    properties.join(", ")
                ),
            )),
            IndexCommand::DropFulltext { name } => Ok(PlanNode::new(
                PlanNodeType::Delete,
                format!("FULLTEXT INDEX {name}"),
            )),
            IndexCommand::ShowFulltext => Ok(PlanNode::new(
                PlanNodeType::ProduceResults,
                "SHOW FULLTEXT INDEXES",
            )),
            IndexCommand::CreateVector {
                name,
                label,
                property,
                dimensions,
                distance,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "VECTOR INDEX {name} FOR :{label}({property}) DIMENSIONS {dimensions} DISTANCE {distance}"
                ),
            )),
            IndexCommand::DropVector { name } => Ok(PlanNode::new(
                PlanNodeType::Delete,
                format!("VECTOR INDEX {name}"),
            )),
            IndexCommand::ShowVector => Ok(PlanNode::new(
                PlanNodeType::ProduceResults,
                "SHOW VECTOR INDEXES",
            )),
        }
    }

    /// 제약조건 명령 실행 계획
    fn plan_constraint(&self, cmd: &ConstraintCommand) -> Result<PlanNode> {
        match cmd {
            ConstraintCommand::CreateUnique {
                name,
                label,
                properties,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR :{} REQUIRE {} IS UNIQUE",
                    name,
                    label,
                    properties.join(", ")
                ),
            )),
            ConstraintCommand::CreateNotNull {
                name,
                label,
                properties,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR :{} REQUIRE {} IS NOT NULL",
                    name,
                    label,
                    properties.join(", ")
                ),
            )),
            ConstraintCommand::CreateNodeKey {
                name,
                label,
                properties,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR :{} REQUIRE ({}) IS NODE KEY",
                    name,
                    label,
                    properties.join(", ")
                ),
            )),
            ConstraintCommand::CreateRange {
                name,
                label,
                property,
                min,
                max,
                ..
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR :{} REQUIRE {} RANGE({:?}, {:?})",
                    name, label, property, min, max
                ),
            )),
            ConstraintCommand::CreateLength {
                name,
                label,
                property,
                min_len,
                max_len,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR :{} REQUIRE {} LENGTH({:?}, {:?})",
                    name, label, property, min_len, max_len
                ),
            )),
            ConstraintCommand::CreatePattern {
                name,
                label,
                property,
                regex,
                ..
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR :{} REQUIRE {} PATTERN('{}')",
                    name, label, property, regex
                ),
            )),
            ConstraintCommand::CreateEnum {
                name,
                label,
                property,
                values,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR :{} REQUIRE {} IN ({:?})",
                    name, label, property, values
                ),
            )),
            ConstraintCommand::CreateImmutable {
                name,
                label,
                property,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR :{} REQUIRE {} IS IMMUTABLE",
                    name, label, property
                ),
            )),
            ConstraintCommand::CreateDefault {
                name,
                label,
                property,
                value,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR :{} REQUIRE {} DEFAULT {:?}",
                    name, label, property, value
                ),
            )),
            ConstraintCommand::CreateCheck {
                name,
                label,
                expression,
                ..
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR :{} REQUIRE CHECK({})",
                    name, label, expression
                ),
            )),
            ConstraintCommand::CreateRequireAtLeastOne {
                name,
                label,
                properties,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR :{} REQUIRE AT LEAST ONE OF ({})",
                    name,
                    label,
                    properties.join(", ")
                ),
            )),
            ConstraintCommand::CreateMutuallyExclusive {
                name,
                label,
                properties,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR :{} REQUIRE MUTUALLY EXCLUSIVE ({})",
                    name,
                    label,
                    properties.join(", ")
                ),
            )),
            ConstraintCommand::CreateCardinality {
                name,
                rel_type,
                min,
                max,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR -[:{}]- REQUIRE CARDINALITY({}, {:?})",
                    name, rel_type, min, max
                ),
            )),
            ConstraintCommand::CreateCascade {
                name,
                rel_type,
                action,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR -[:{}]- REQUIRE ON DELETE {:?}",
                    name, rel_type, action
                ),
            )),
            ConstraintCommand::CreateReferentialIntegrity {
                name,
                rel_type,
                on_delete,
                on_update,
            } => Ok(PlanNode::new(
                PlanNodeType::Create,
                format!(
                    "CONSTRAINT {} FOR -[:{}]- REQUIRE ON DELETE {:?} ON UPDATE {:?}",
                    name, rel_type, on_delete, on_update
                ),
            )),
            ConstraintCommand::Drop { name } => Ok(PlanNode::new(
                PlanNodeType::Delete,
                format!("CONSTRAINT {}", name),
            )),
            ConstraintCommand::Show => Ok(PlanNode::new(
                PlanNodeType::ProduceResults,
                "SHOW CONSTRAINTS",
            )),
            ConstraintCommand::ShowFor { label } => Ok(PlanNode::new(
                PlanNodeType::ProduceResults,
                format!("SHOW CONSTRAINTS FOR :{}", label),
            )),
        }
    }

    /// 관리 명령 실행 계획
    fn plan_admin(&self, cmd: &AdminCommand) -> Result<PlanNode> {
        use AdminCommand::*;
        match cmd {
            CreateUser {
                name,
                if_not_exists,
                ..
            } => {
                let prefix = if *if_not_exists { "IF NOT EXISTS " } else { "" };
                Ok(PlanNode::new(
                    PlanNodeType::Admin,
                    format!("CREATE USER {}{}", prefix, name),
                ))
            }
            DropUser { name, if_exists } => {
                let suffix = if *if_exists { " IF EXISTS" } else { "" };
                Ok(PlanNode::new(
                    PlanNodeType::Admin,
                    format!("DROP USER {}{}", name, suffix),
                ))
            }
            AlterUser { name, .. } => Ok(PlanNode::new(
                PlanNodeType::Admin,
                format!("ALTER USER {}", name),
            )),
            ShowUsers => Ok(PlanNode::new(PlanNodeType::Admin, "SHOW USERS".to_string())),
            ShowCurrentUser => Ok(PlanNode::new(
                PlanNodeType::Admin,
                "SHOW CURRENT USER".to_string(),
            )),
            CreateDatabase {
                name,
                if_not_exists,
            } => {
                let prefix = if *if_not_exists { "IF NOT EXISTS " } else { "" };
                Ok(PlanNode::new(
                    PlanNodeType::Admin,
                    format!("CREATE DATABASE {}{}", prefix, name),
                ))
            }
            DropDatabase { name, if_exists } => {
                let suffix = if *if_exists { " IF EXISTS" } else { "" };
                Ok(PlanNode::new(
                    PlanNodeType::Admin,
                    format!("DROP DATABASE {}{}", name, suffix),
                ))
            }
            ShowDatabases => Ok(PlanNode::new(
                PlanNodeType::Admin,
                "SHOW DATABASES".to_string(),
            )),
            ShowDefaultDatabase => Ok(PlanNode::new(
                PlanNodeType::Admin,
                "SHOW DEFAULT DATABASE".to_string(),
            )),
            StartDatabase { name } => Ok(PlanNode::new(
                PlanNodeType::Admin,
                format!("START DATABASE {}", name),
            )),
            StopDatabase { name } => Ok(PlanNode::new(
                PlanNodeType::Admin,
                format!("STOP DATABASE {}", name),
            )),
            ShowProcedures => Ok(PlanNode::new(
                PlanNodeType::Admin,
                "SHOW PROCEDURES".to_string(),
            )),
        }
    }

    /// CALL 서브쿼리 실행 계획
    pub(crate) fn plan_call(&self, call: &CallSubquery) -> Result<PlanNode> {
        let subplan = self.plan_single_query(&call.subquery)?;
        Ok(PlanNode::new(PlanNodeType::Call, "subquery").with_child(subplan))
    }

    /// 프로시저 호출 실행 계획
    pub(crate) fn plan_procedure(&self, proc_call: &ProcedureCall) -> Result<PlanNode> {
        let yields = if let Some(yields) = &proc_call.yields {
            yields
                .iter()
                .map(|y| {
                    if let Some(alias) = &y.alias {
                        format!("{} AS {}", y.variable, alias)
                    } else {
                        y.variable.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            String::new()
        };

        let description = if yields.is_empty() {
            format!("CALL {}", proc_call.name)
        } else {
            format!("CALL {} YIELD {}", proc_call.name, yields)
        };

        Ok(PlanNode::new(PlanNodeType::Call, description))
    }
}
