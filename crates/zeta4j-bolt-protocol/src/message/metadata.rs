//! Bolt protocol metadata types.
//!
//! Additional metadata structures used in Bolt messages.

use std::collections::HashMap;

use crate::packstream::PackStreamValue;

/// Query statistics returned in SUCCESS after PULL.
#[derive(Debug, Clone, Default)]
pub struct QueryStats {
    /// Nodes created
    pub nodes_created: i64,
    /// Nodes deleted
    pub nodes_deleted: i64,
    /// Relationships created
    pub relationships_created: i64,
    /// Relationships deleted
    pub relationships_deleted: i64,
    /// Properties set
    pub properties_set: i64,
    /// Labels added
    pub labels_added: i64,
    /// Labels removed
    pub labels_removed: i64,
    /// Indexes added
    pub indexes_added: i64,
    /// Indexes removed
    pub indexes_removed: i64,
    /// Constraints added
    pub constraints_added: i64,
    /// Constraints removed
    pub constraints_removed: i64,
    /// Contains system updates
    pub contains_system_updates: bool,
    /// Contains updates
    pub contains_updates: bool,
}

impl QueryStats {
    /// Create empty stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if there were any modifications.
    pub fn has_updates(&self) -> bool {
        self.contains_updates
            || self.contains_system_updates
            || self.nodes_created > 0
            || self.nodes_deleted > 0
            || self.relationships_created > 0
            || self.relationships_deleted > 0
            || self.properties_set > 0
            || self.labels_added > 0
            || self.labels_removed > 0
            || self.indexes_added > 0
            || self.indexes_removed > 0
            || self.constraints_added > 0
            || self.constraints_removed > 0
    }

    /// Convert to PackStream map.
    pub fn to_map(&self) -> HashMap<String, PackStreamValue> {
        let mut map = HashMap::new();

        if self.nodes_created > 0 {
            map.insert(
                "nodes-created".to_string(),
                PackStreamValue::Integer(self.nodes_created),
            );
        }
        if self.nodes_deleted > 0 {
            map.insert(
                "nodes-deleted".to_string(),
                PackStreamValue::Integer(self.nodes_deleted),
            );
        }
        if self.relationships_created > 0 {
            map.insert(
                "relationships-created".to_string(),
                PackStreamValue::Integer(self.relationships_created),
            );
        }
        if self.relationships_deleted > 0 {
            map.insert(
                "relationships-deleted".to_string(),
                PackStreamValue::Integer(self.relationships_deleted),
            );
        }
        if self.properties_set > 0 {
            map.insert(
                "properties-set".to_string(),
                PackStreamValue::Integer(self.properties_set),
            );
        }
        if self.labels_added > 0 {
            map.insert(
                "labels-added".to_string(),
                PackStreamValue::Integer(self.labels_added),
            );
        }
        if self.labels_removed > 0 {
            map.insert(
                "labels-removed".to_string(),
                PackStreamValue::Integer(self.labels_removed),
            );
        }
        if self.indexes_added > 0 {
            map.insert(
                "indexes-added".to_string(),
                PackStreamValue::Integer(self.indexes_added),
            );
        }
        if self.indexes_removed > 0 {
            map.insert(
                "indexes-removed".to_string(),
                PackStreamValue::Integer(self.indexes_removed),
            );
        }
        if self.constraints_added > 0 {
            map.insert(
                "constraints-added".to_string(),
                PackStreamValue::Integer(self.constraints_added),
            );
        }
        if self.constraints_removed > 0 {
            map.insert(
                "constraints-removed".to_string(),
                PackStreamValue::Integer(self.constraints_removed),
            );
        }
        if self.contains_system_updates {
            map.insert(
                "contains-system-updates".to_string(),
                PackStreamValue::Boolean(true),
            );
        }
        if self.contains_updates {
            map.insert(
                "contains-updates".to_string(),
                PackStreamValue::Boolean(true),
            );
        }

        map
    }

    /// Parse from PackStream map.
    pub fn from_map(map: &HashMap<String, PackStreamValue>) -> Self {
        Self {
            nodes_created: map
                .get("nodes-created")
                .and_then(|v| v.as_int())
                .unwrap_or(0),
            nodes_deleted: map
                .get("nodes-deleted")
                .and_then(|v| v.as_int())
                .unwrap_or(0),
            relationships_created: map
                .get("relationships-created")
                .and_then(|v| v.as_int())
                .unwrap_or(0),
            relationships_deleted: map
                .get("relationships-deleted")
                .and_then(|v| v.as_int())
                .unwrap_or(0),
            properties_set: map
                .get("properties-set")
                .and_then(|v| v.as_int())
                .unwrap_or(0),
            labels_added: map
                .get("labels-added")
                .and_then(|v| v.as_int())
                .unwrap_or(0),
            labels_removed: map
                .get("labels-removed")
                .and_then(|v| v.as_int())
                .unwrap_or(0),
            indexes_added: map
                .get("indexes-added")
                .and_then(|v| v.as_int())
                .unwrap_or(0),
            indexes_removed: map
                .get("indexes-removed")
                .and_then(|v| v.as_int())
                .unwrap_or(0),
            constraints_added: map
                .get("constraints-added")
                .and_then(|v| v.as_int())
                .unwrap_or(0),
            constraints_removed: map
                .get("constraints-removed")
                .and_then(|v| v.as_int())
                .unwrap_or(0),
            contains_system_updates: map
                .get("contains-system-updates")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            contains_updates: map
                .get("contains-updates")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }
    }
}

/// Query plan returned in SUCCESS when EXPLAIN is used.
#[derive(Debug, Clone)]
pub struct QueryPlan {
    /// Operator type
    pub operator_type: String,
    /// Arguments
    pub args: HashMap<String, PackStreamValue>,
    /// Identifiers
    pub identifiers: Vec<String>,
    /// Child plans
    pub children: Vec<QueryPlan>,
}

impl QueryPlan {
    /// Create a new query plan node.
    pub fn new(operator_type: &str) -> Self {
        Self {
            operator_type: operator_type.to_string(),
            args: HashMap::new(),
            identifiers: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Add an argument.
    pub fn with_arg(mut self, key: &str, value: PackStreamValue) -> Self {
        self.args.insert(key.to_string(), value);
        self
    }

    /// Add identifiers.
    pub fn with_identifiers(mut self, ids: Vec<String>) -> Self {
        self.identifiers = ids;
        self
    }

    /// Add a child plan.
    pub fn with_child(mut self, child: QueryPlan) -> Self {
        self.children.push(child);
        self
    }

    /// Convert to PackStream map.
    pub fn to_map(&self) -> HashMap<String, PackStreamValue> {
        let mut map = HashMap::new();
        map.insert(
            "operatorType".to_string(),
            PackStreamValue::String(self.operator_type.clone()),
        );
        map.insert("args".to_string(), PackStreamValue::Map(self.args.clone()));

        let ids: Vec<PackStreamValue> = self
            .identifiers
            .iter()
            .map(|s| PackStreamValue::String(s.clone()))
            .collect();
        map.insert("identifiers".to_string(), PackStreamValue::List(ids));

        let children: Vec<PackStreamValue> = self
            .children
            .iter()
            .map(|c| PackStreamValue::Map(c.to_map()))
            .collect();
        map.insert("children".to_string(), PackStreamValue::List(children));

        map
    }

    /// Parse from PackStream map.
    pub fn from_map(map: &HashMap<String, PackStreamValue>) -> Option<Self> {
        let operator_type = map
            .get("operatorType")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())?;

        let args = map
            .get("args")
            .and_then(|v| v.as_map())
            .cloned()
            .unwrap_or_default();

        let identifiers = map
            .get("identifiers")
            .and_then(|v| {
                if let PackStreamValue::List(list) = v {
                    Some(
                        list.iter()
                            .filter_map(|item| item.as_str().map(|s| s.to_string()))
                            .collect(),
                    )
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let children = map
            .get("children")
            .and_then(|v| {
                if let PackStreamValue::List(list) = v {
                    Some(
                        list.iter()
                            .filter_map(|item| {
                                if let PackStreamValue::Map(m) = item {
                                    QueryPlan::from_map(m)
                                } else {
                                    None
                                }
                            })
                            .collect(),
                    )
                } else {
                    None
                }
            })
            .unwrap_or_default();

        Some(Self {
            operator_type,
            args,
            identifiers,
            children,
        })
    }
}

/// Profile information returned in SUCCESS when PROFILE is used.
#[derive(Debug, Clone)]
pub struct QueryProfile {
    /// Base plan
    pub plan: QueryPlan,
    /// Rows processed
    pub db_hits: i64,
    /// Rows output
    pub rows: i64,
    /// Page cache hits
    pub page_cache_hits: i64,
    /// Page cache misses
    pub page_cache_misses: i64,
    /// Time in microseconds
    pub time: i64,
}

impl QueryProfile {
    /// Create a new profile.
    pub fn new(plan: QueryPlan) -> Self {
        Self {
            plan,
            db_hits: 0,
            rows: 0,
            page_cache_hits: 0,
            page_cache_misses: 0,
            time: 0,
        }
    }

    /// Convert to PackStream map.
    pub fn to_map(&self) -> HashMap<String, PackStreamValue> {
        let mut map = self.plan.to_map();
        map.insert("dbHits".to_string(), PackStreamValue::Integer(self.db_hits));
        map.insert("rows".to_string(), PackStreamValue::Integer(self.rows));
        map.insert(
            "pageCacheHits".to_string(),
            PackStreamValue::Integer(self.page_cache_hits),
        );
        map.insert(
            "pageCacheMisses".to_string(),
            PackStreamValue::Integer(self.page_cache_misses),
        );
        map.insert("time".to_string(), PackStreamValue::Integer(self.time));
        map
    }
}

/// Notification returned in SUCCESS metadata.
#[derive(Debug, Clone)]
pub struct Notification {
    /// Notification code
    pub code: String,
    /// Title
    pub title: String,
    /// Description
    pub description: String,
    /// Severity
    pub severity: NotificationSeverity,
    /// Position in query
    pub position: Option<NotificationPosition>,
}

/// Notification severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationSeverity {
    /// Warning
    Warning,
    /// Information
    Information,
}

impl NotificationSeverity {
    /// Convert to string.
    pub fn as_str(&self) -> &'static str {
        match self {
            NotificationSeverity::Warning => "WARNING",
            NotificationSeverity::Information => "INFORMATION",
        }
    }

    /// Parse from string.
    pub fn parse(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "WARNING" => NotificationSeverity::Warning,
            _ => NotificationSeverity::Information,
        }
    }
}

/// Position in query for notification.
#[derive(Debug, Clone, Copy)]
pub struct NotificationPosition {
    /// Line number (1-based)
    pub line: i64,
    /// Column number (1-based)
    pub column: i64,
    /// Offset from start
    pub offset: i64,
}

impl Notification {
    /// Create a new notification.
    pub fn new(code: &str, title: &str, description: &str, severity: NotificationSeverity) -> Self {
        Self {
            code: code.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            severity,
            position: None,
        }
    }

    /// Create a deprecation warning.
    pub fn deprecation_warning(feature: &str, replacement: Option<&str>) -> Self {
        let desc = if let Some(repl) = replacement {
            format!("'{}' is deprecated. Use '{}' instead.", feature, repl)
        } else {
            format!("'{}' is deprecated.", feature)
        };

        Self::new(
            "Zeta.ClientNotification.Statement.FeatureDeprecationWarning",
            "Feature Deprecated",
            &desc,
            NotificationSeverity::Warning,
        )
    }

    /// Create a performance hint.
    pub fn performance_hint(hint: &str) -> Self {
        Self::new(
            "Zeta.ClientNotification.Statement.CartesianProduct",
            "Performance Hint",
            hint,
            NotificationSeverity::Warning,
        )
    }

    /// Set position.
    pub fn with_position(mut self, line: i64, column: i64, offset: i64) -> Self {
        self.position = Some(NotificationPosition {
            line,
            column,
            offset,
        });
        self
    }

    /// Convert to PackStream map.
    pub fn to_map(&self) -> HashMap<String, PackStreamValue> {
        let mut map = HashMap::new();
        map.insert(
            "code".to_string(),
            PackStreamValue::String(self.code.clone()),
        );
        map.insert(
            "title".to_string(),
            PackStreamValue::String(self.title.clone()),
        );
        map.insert(
            "description".to_string(),
            PackStreamValue::String(self.description.clone()),
        );
        map.insert(
            "severity".to_string(),
            PackStreamValue::String(self.severity.as_str().to_string()),
        );

        if let Some(pos) = &self.position {
            let mut pos_map = HashMap::new();
            pos_map.insert("line".to_string(), PackStreamValue::Integer(pos.line));
            pos_map.insert("column".to_string(), PackStreamValue::Integer(pos.column));
            pos_map.insert("offset".to_string(), PackStreamValue::Integer(pos.offset));
            map.insert("position".to_string(), PackStreamValue::Map(pos_map));
        }

        map
    }

    /// Parse from PackStream map.
    pub fn from_map(map: &HashMap<String, PackStreamValue>) -> Option<Self> {
        let code = map
            .get("code")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())?;
        let title = map
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let description = map
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let severity = map
            .get("severity")
            .and_then(|v| v.as_str())
            .map(NotificationSeverity::parse)
            .unwrap_or(NotificationSeverity::Information);

        let position = map.get("position").and_then(|v| {
            if let PackStreamValue::Map(pos_map) = v {
                let line = pos_map.get("line").and_then(|v| v.as_int())?;
                let column = pos_map.get("column").and_then(|v| v.as_int())?;
                let offset = pos_map.get("offset").and_then(|v| v.as_int()).unwrap_or(0);
                Some(NotificationPosition {
                    line,
                    column,
                    offset,
                })
            } else {
                None
            }
        });

        Some(Self {
            code,
            title,
            description,
            severity,
            position,
        })
    }
}

/// Routing table returned in SUCCESS after ROUTE.
#[derive(Debug, Clone)]
pub struct RoutingTable {
    /// Time to live in seconds
    pub ttl: i64,
    /// Database name
    pub db: String,
    /// Servers
    pub servers: Vec<RoutingServer>,
}

/// Server role in routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerRole {
    /// Read server
    Read,
    /// Write server
    Write,
    /// Route server (for routing queries)
    Route,
}

impl ServerRole {
    /// Convert to string.
    pub fn as_str(&self) -> &'static str {
        match self {
            ServerRole::Read => "READ",
            ServerRole::Write => "WRITE",
            ServerRole::Route => "ROUTE",
        }
    }

    /// Parse from string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "READ" => Some(ServerRole::Read),
            "WRITE" => Some(ServerRole::Write),
            "ROUTE" => Some(ServerRole::Route),
            _ => None,
        }
    }
}

/// Server in routing table.
#[derive(Debug, Clone)]
pub struct RoutingServer {
    /// Server addresses
    pub addresses: Vec<String>,
    /// Server role
    pub role: ServerRole,
}

impl RoutingTable {
    /// Create a new routing table.
    pub fn new(db: &str, ttl: i64) -> Self {
        Self {
            ttl,
            db: db.to_string(),
            servers: Vec::new(),
        }
    }

    /// Add a server.
    pub fn add_server(&mut self, role: ServerRole, addresses: Vec<String>) {
        self.servers.push(RoutingServer { addresses, role });
    }

    /// Convert to PackStream map.
    pub fn to_map(&self) -> HashMap<String, PackStreamValue> {
        let mut map = HashMap::new();
        map.insert("ttl".to_string(), PackStreamValue::Integer(self.ttl));
        map.insert("db".to_string(), PackStreamValue::String(self.db.clone()));

        let servers: Vec<PackStreamValue> = self
            .servers
            .iter()
            .map(|s| {
                let mut server_map = HashMap::new();
                let addrs: Vec<PackStreamValue> = s
                    .addresses
                    .iter()
                    .map(|a| PackStreamValue::String(a.clone()))
                    .collect();
                server_map.insert("addresses".to_string(), PackStreamValue::List(addrs));
                server_map.insert(
                    "role".to_string(),
                    PackStreamValue::String(s.role.as_str().to_string()),
                );
                PackStreamValue::Map(server_map)
            })
            .collect();
        map.insert("servers".to_string(), PackStreamValue::List(servers));

        map
    }
}
