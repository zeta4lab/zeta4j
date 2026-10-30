//! Bolt protocol request messages.
//!
//! Request messages are sent from the client to the server.

use std::collections::HashMap;
use std::time::Duration;

use super::tag;
use crate::packstream::{PackStreamError, PackStreamStructure, PackStreamValue};

/// Access mode for transactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccessMode {
    /// Read-write access (default)
    #[default]
    Write,
    /// Read-only access
    Read,
}

impl AccessMode {
    /// Convert from string.
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "r" | "read" => AccessMode::Read,
            _ => AccessMode::Write,
        }
    }

    /// Convert to string for metadata.
    pub fn as_str(&self) -> &'static str {
        match self {
            AccessMode::Read => "r",
            AccessMode::Write => "w",
        }
    }
}

/// Authentication token for HELLO message.
#[derive(Debug, Clone)]
pub struct AuthToken {
    /// Authentication scheme (e.g., "basic", "bearer")
    pub scheme: String,
    /// Principal (username)
    pub principal: Option<String>,
    /// Credentials (password)
    pub credentials: Option<String>,
    /// Realm
    pub realm: Option<String>,
    /// Additional parameters
    pub parameters: HashMap<String, PackStreamValue>,
}

impl AuthToken {
    /// Create a basic auth token.
    pub fn basic(principal: &str, credentials: &str) -> Self {
        Self {
            scheme: "basic".to_string(),
            principal: Some(principal.to_string()),
            credentials: Some(credentials.to_string()),
            realm: None,
            parameters: HashMap::new(),
        }
    }

    pub fn bearer(token: &str) -> Self {
        Self {
            scheme: "bearer".to_string(),
            principal: None,
            credentials: Some(token.to_string()),
            realm: None,
            parameters: HashMap::new(),
        }
    }

    /// Create an anonymous auth token (no auth).
    pub fn none() -> Self {
        Self {
            scheme: "none".to_string(),
            principal: None,
            credentials: None,
            realm: None,
            parameters: HashMap::new(),
        }
    }

    /// Convert to PackStream map.
    pub fn to_map(&self) -> HashMap<String, PackStreamValue> {
        let mut map = HashMap::new();
        map.insert(
            "scheme".to_string(),
            PackStreamValue::String(self.scheme.clone()),
        );
        if let Some(ref p) = self.principal {
            map.insert("principal".to_string(), PackStreamValue::String(p.clone()));
        }
        if let Some(ref c) = self.credentials {
            map.insert(
                "credentials".to_string(),
                PackStreamValue::String(c.clone()),
            );
        }
        if let Some(ref r) = self.realm {
            map.insert("realm".to_string(), PackStreamValue::String(r.clone()));
        }
        for (k, v) in &self.parameters {
            map.insert(k.clone(), v.clone());
        }
        map
    }

    /// Parse from PackStream map.
    pub fn from_map(map: &HashMap<String, PackStreamValue>) -> Option<Self> {
        let scheme = map
            .get("scheme")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "none".to_string());

        let principal = map
            .get("principal")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let credentials = map
            .get("credentials")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let realm = map
            .get("realm")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut parameters = HashMap::new();
        for (k, v) in map {
            if !["scheme", "principal", "credentials", "realm"].contains(&k.as_str()) {
                parameters.insert(k.clone(), v.clone());
            }
        }

        Some(Self {
            scheme,
            principal,
            credentials,
            realm,
            parameters,
        })
    }
}

/// All Bolt request messages.
#[derive(Debug, Clone)]
pub enum BoltRequest {
    /// HELLO - Initialize connection
    Hello(HelloMessage),
    /// GOODBYE - Close connection gracefully
    Goodbye,
    /// RESET - Reset connection state
    Reset,
    /// RUN - Execute a query
    Run(RunMessage),
    /// PULL - Pull results
    Pull(PullMessage),
    /// DISCARD - Discard results
    Discard(DiscardMessage),
    /// BEGIN - Start transaction
    Begin(BeginMessage),
    /// COMMIT - Commit transaction
    Commit,
    /// ROLLBACK - Rollback transaction
    Rollback,
    /// ROUTE - Request routing information (Bolt 4.3+)
    Route(RouteMessage),
    /// LOGON - Re-authenticate (Bolt 5.1+)
    Logon(LogonMessage),
    /// LOGOFF - Deauthenticate (Bolt 5.1+)
    Logoff,
}

impl BoltRequest {
    /// Get the message tag.
    pub fn tag(&self) -> u8 {
        match self {
            BoltRequest::Hello(_) => tag::HELLO,
            BoltRequest::Goodbye => tag::GOODBYE,
            BoltRequest::Reset => tag::RESET,
            BoltRequest::Run(_) => tag::RUN,
            BoltRequest::Pull(_) => tag::PULL,
            BoltRequest::Discard(_) => tag::DISCARD,
            BoltRequest::Begin(_) => tag::BEGIN,
            BoltRequest::Commit => tag::COMMIT,
            BoltRequest::Rollback => tag::ROLLBACK,
            BoltRequest::Route(_) => tag::ROUTE,
            BoltRequest::Logon(_) => tag::LOGON,
            BoltRequest::Logoff => tag::LOGOFF,
        }
    }

    /// Get message name for logging.
    pub fn name(&self) -> &'static str {
        match self {
            BoltRequest::Hello(_) => "HELLO",
            BoltRequest::Goodbye => "GOODBYE",
            BoltRequest::Reset => "RESET",
            BoltRequest::Run(_) => "RUN",
            BoltRequest::Pull(_) => "PULL",
            BoltRequest::Discard(_) => "DISCARD",
            BoltRequest::Begin(_) => "BEGIN",
            BoltRequest::Commit => "COMMIT",
            BoltRequest::Rollback => "ROLLBACK",
            BoltRequest::Route(_) => "ROUTE",
            BoltRequest::Logon(_) => "LOGON",
            BoltRequest::Logoff => "LOGOFF",
        }
    }

    /// Convert to PackStream structure.
    pub fn to_structure(&self) -> PackStreamStructure {
        match self {
            BoltRequest::Hello(msg) => msg.to_structure(),
            BoltRequest::Goodbye => PackStreamStructure::new(tag::GOODBYE, vec![]),
            BoltRequest::Reset => PackStreamStructure::new(tag::RESET, vec![]),
            BoltRequest::Run(msg) => msg.to_structure(),
            BoltRequest::Pull(msg) => msg.to_structure(),
            BoltRequest::Discard(msg) => msg.to_structure(),
            BoltRequest::Begin(msg) => msg.to_structure(),
            BoltRequest::Commit => PackStreamStructure::new(tag::COMMIT, vec![]),
            BoltRequest::Rollback => PackStreamStructure::new(tag::ROLLBACK, vec![]),
            BoltRequest::Route(msg) => msg.to_structure(),
            BoltRequest::Logon(msg) => msg.to_structure(),
            BoltRequest::Logoff => PackStreamStructure::new(tag::LOGOFF, vec![]),
        }
    }

    /// Parse from PackStream structure.
    pub fn from_structure(s: &PackStreamStructure) -> Result<Self, PackStreamError> {
        match s.tag {
            tag::HELLO => Ok(BoltRequest::Hello(HelloMessage::from_structure(s)?)),
            tag::GOODBYE => Ok(BoltRequest::Goodbye),
            tag::RESET => Ok(BoltRequest::Reset),
            tag::RUN => Ok(BoltRequest::Run(RunMessage::from_structure(s)?)),
            tag::PULL => Ok(BoltRequest::Pull(PullMessage::from_structure(s)?)),
            tag::DISCARD => Ok(BoltRequest::Discard(DiscardMessage::from_structure(s)?)),
            tag::BEGIN => Ok(BoltRequest::Begin(BeginMessage::from_structure(s)?)),
            tag::COMMIT => Ok(BoltRequest::Commit),
            tag::ROLLBACK => Ok(BoltRequest::Rollback),
            tag::ROUTE => Ok(BoltRequest::Route(RouteMessage::from_structure(s)?)),
            tag::LOGON => Ok(BoltRequest::Logon(LogonMessage::from_structure(s)?)),
            tag::LOGOFF => Ok(BoltRequest::Logoff),
            _ => Err(PackStreamError::InvalidStructure(format!(
                "Unknown request message tag: 0x{:02X}",
                s.tag
            ))),
        }
    }
}

/// HELLO message - Initialize connection.
#[derive(Debug, Clone)]
pub struct HelloMessage {
    /// User agent string
    pub user_agent: String,
    /// Authentication token
    pub auth: Option<AuthToken>,
    /// Routing context
    pub routing: Option<HashMap<String, PackStreamValue>>,
    /// Patch bolt (Bolt 5.1+)
    pub patch_bolt: Option<Vec<String>>,
    /// Additional extra data
    pub extra: HashMap<String, PackStreamValue>,
}

impl HelloMessage {
    /// Create a new HELLO message.
    pub fn new(user_agent: &str) -> Self {
        Self {
            user_agent: user_agent.to_string(),
            auth: None,
            routing: None,
            patch_bolt: None,
            extra: HashMap::new(),
        }
    }

    /// Set authentication.
    pub fn with_auth(mut self, auth: AuthToken) -> Self {
        self.auth = Some(auth);
        self
    }

    /// Set routing context.
    pub fn with_routing(mut self, routing: HashMap<String, PackStreamValue>) -> Self {
        self.routing = Some(routing);
        self
    }

    /// Convert to PackStream structure.
    pub fn to_structure(&self) -> PackStreamStructure {
        let mut extra = self.extra.clone();
        extra.insert(
            "user_agent".to_string(),
            PackStreamValue::String(self.user_agent.clone()),
        );
        if let Some(ref auth) = self.auth {
            for (k, v) in auth.to_map() {
                extra.insert(k, v);
            }
        }
        if let Some(ref routing) = self.routing {
            extra.insert("routing".to_string(), PackStreamValue::Map(routing.clone()));
        }
        if let Some(ref patches) = self.patch_bolt {
            let list: Vec<PackStreamValue> = patches
                .iter()
                .map(|s| PackStreamValue::String(s.clone()))
                .collect();
            extra.insert("patch_bolt".to_string(), PackStreamValue::List(list));
        }

        PackStreamStructure::new(tag::HELLO, vec![PackStreamValue::Map(extra)])
    }

    /// Parse from PackStream structure.
    pub fn from_structure(s: &PackStreamStructure) -> Result<Self, PackStreamError> {
        if s.tag != tag::HELLO {
            return Err(PackStreamError::InvalidStructure(
                "Expected HELLO tag".to_string(),
            ));
        }
        if s.fields.is_empty() {
            return Err(PackStreamError::InvalidStructure(
                "HELLO requires extra map".to_string(),
            ));
        }

        let extra = s.fields[0].as_map().ok_or_else(|| {
            PackStreamError::InvalidStructure("HELLO extra must be map".to_string())
        })?;

        let user_agent = extra
            .get("user_agent")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        let auth = AuthToken::from_map(extra);

        let routing = extra.get("routing").and_then(|v| v.as_map()).cloned();

        let patch_bolt = extra.get("patch_bolt").and_then(|v| {
            if let PackStreamValue::List(list) = v {
                Some(
                    list.iter()
                        .filter_map(|item| item.as_str().map(|s| s.to_string()))
                        .collect(),
                )
            } else {
                None
            }
        });

        let mut remaining = HashMap::new();
        for (k, v) in extra {
            if ![
                "user_agent",
                "scheme",
                "principal",
                "credentials",
                "realm",
                "routing",
                "patch_bolt",
            ]
            .contains(&k.as_str())
            {
                remaining.insert(k.clone(), v.clone());
            }
        }

        Ok(Self {
            user_agent,
            auth,
            routing,
            patch_bolt,
            extra: remaining,
        })
    }
}

/// RUN message - Execute a query.
#[derive(Debug, Clone)]
pub struct RunMessage {
    /// Cypher query string
    pub query: String,
    /// Query parameters
    pub parameters: HashMap<String, PackStreamValue>,
    /// Extra metadata
    pub extra: HashMap<String, PackStreamValue>,
}

impl RunMessage {
    /// Create a new RUN message.
    pub fn new(query: &str) -> Self {
        Self {
            query: query.to_string(),
            parameters: HashMap::new(),
            extra: HashMap::new(),
        }
    }

    /// Set query parameters.
    pub fn with_parameters(mut self, params: HashMap<String, PackStreamValue>) -> Self {
        self.parameters = params;
        self
    }

    /// Set extra metadata.
    pub fn with_extra(mut self, extra: HashMap<String, PackStreamValue>) -> Self {
        self.extra = extra;
        self
    }

    /// Set database name.
    pub fn with_database(mut self, db: &str) -> Self {
        self.extra
            .insert("db".to_string(), PackStreamValue::String(db.to_string()));
        self
    }

    /// Set bookmark(s).
    pub fn with_bookmarks(mut self, bookmarks: Vec<String>) -> Self {
        let list: Vec<PackStreamValue> =
            bookmarks.into_iter().map(PackStreamValue::String).collect();
        self.extra
            .insert("bookmarks".to_string(), PackStreamValue::List(list));
        self
    }

    /// Convert to PackStream structure.
    pub fn to_structure(&self) -> PackStreamStructure {
        PackStreamStructure::new(
            tag::RUN,
            vec![
                PackStreamValue::String(self.query.clone()),
                PackStreamValue::Map(self.parameters.clone()),
                PackStreamValue::Map(self.extra.clone()),
            ],
        )
    }

    /// Parse from PackStream structure.
    pub fn from_structure(s: &PackStreamStructure) -> Result<Self, PackStreamError> {
        if s.tag != tag::RUN {
            return Err(PackStreamError::InvalidStructure(
                "Expected RUN tag".to_string(),
            ));
        }
        if s.fields.len() < 2 {
            return Err(PackStreamError::InvalidStructure(
                "RUN requires query and parameters".to_string(),
            ));
        }

        let query = s.fields[0]
            .as_str()
            .ok_or_else(|| {
                PackStreamError::InvalidStructure("RUN query must be string".to_string())
            })?
            .to_string();

        let parameters = s.fields[1]
            .as_map()
            .ok_or_else(|| {
                PackStreamError::InvalidStructure("RUN parameters must be map".to_string())
            })?
            .clone();

        let extra = if s.fields.len() > 2 {
            s.fields[2].as_map().cloned().unwrap_or_default()
        } else {
            HashMap::new()
        };

        Ok(Self {
            query,
            parameters,
            extra,
        })
    }
}

/// PULL message - Pull query results.
#[derive(Debug, Clone)]
pub struct PullMessage {
    /// Number of records to pull (-1 for all)
    pub n: i64,
    /// Query ID for multi-query results
    pub qid: Option<i64>,
}

impl PullMessage {
    /// Create a PULL ALL message.
    pub fn all() -> Self {
        Self { n: -1, qid: None }
    }

    /// Create a PULL with specific count.
    pub fn with_n(n: i64) -> Self {
        Self { n, qid: None }
    }

    /// Set query ID.
    pub fn with_qid(mut self, qid: i64) -> Self {
        self.qid = Some(qid);
        self
    }

    /// Convert to PackStream structure.
    pub fn to_structure(&self) -> PackStreamStructure {
        let mut extra = HashMap::new();
        extra.insert("n".to_string(), PackStreamValue::Integer(self.n));
        if let Some(qid) = self.qid {
            extra.insert("qid".to_string(), PackStreamValue::Integer(qid));
        }

        PackStreamStructure::new(tag::PULL, vec![PackStreamValue::Map(extra)])
    }

    /// Parse from PackStream structure.
    pub fn from_structure(s: &PackStreamStructure) -> Result<Self, PackStreamError> {
        if s.tag != tag::PULL {
            return Err(PackStreamError::InvalidStructure(
                "Expected PULL tag".to_string(),
            ));
        }

        // Bolt 4.0+ uses extra map
        let (n, qid) = if !s.fields.is_empty() {
            if let Some(extra) = s.fields[0].as_map() {
                let n = extra.get("n").and_then(|v| v.as_int()).unwrap_or(-1);
                let qid = extra.get("qid").and_then(|v| v.as_int());
                (n, qid)
            } else {
                (-1, None)
            }
        } else {
            (-1, None)
        };

        Ok(Self { n, qid })
    }
}

/// DISCARD message - Discard query results.
#[derive(Debug, Clone)]
pub struct DiscardMessage {
    /// Number of records to discard (-1 for all)
    pub n: i64,
    /// Query ID for multi-query results
    pub qid: Option<i64>,
}

impl DiscardMessage {
    /// Create a DISCARD ALL message.
    pub fn all() -> Self {
        Self { n: -1, qid: None }
    }

    /// Create a DISCARD with specific count.
    pub fn with_n(n: i64) -> Self {
        Self { n, qid: None }
    }

    /// Set query ID.
    pub fn with_qid(mut self, qid: i64) -> Self {
        self.qid = Some(qid);
        self
    }

    /// Convert to PackStream structure.
    pub fn to_structure(&self) -> PackStreamStructure {
        let mut extra = HashMap::new();
        extra.insert("n".to_string(), PackStreamValue::Integer(self.n));
        if let Some(qid) = self.qid {
            extra.insert("qid".to_string(), PackStreamValue::Integer(qid));
        }

        PackStreamStructure::new(tag::DISCARD, vec![PackStreamValue::Map(extra)])
    }

    /// Parse from PackStream structure.
    pub fn from_structure(s: &PackStreamStructure) -> Result<Self, PackStreamError> {
        if s.tag != tag::DISCARD {
            return Err(PackStreamError::InvalidStructure(
                "Expected DISCARD tag".to_string(),
            ));
        }

        let (n, qid) = if !s.fields.is_empty() {
            if let Some(extra) = s.fields[0].as_map() {
                let n = extra.get("n").and_then(|v| v.as_int()).unwrap_or(-1);
                let qid = extra.get("qid").and_then(|v| v.as_int());
                (n, qid)
            } else {
                (-1, None)
            }
        } else {
            (-1, None)
        };

        Ok(Self { n, qid })
    }
}

/// BEGIN message - Start a transaction.
#[derive(Debug, Clone)]
pub struct BeginMessage {
    /// Bookmarks to wait for
    pub bookmarks: Vec<String>,
    /// Transaction timeout
    pub tx_timeout: Option<Duration>,
    /// Access mode (read/write)
    pub mode: AccessMode,
    /// Database name
    pub database: Option<String>,
    /// Transaction metadata
    pub tx_metadata: HashMap<String, PackStreamValue>,
}

impl BeginMessage {
    /// Create a new BEGIN message.
    pub fn new() -> Self {
        Self {
            bookmarks: Vec::new(),
            tx_timeout: None,
            mode: AccessMode::Write,
            database: None,
            tx_metadata: HashMap::new(),
        }
    }

    /// Set bookmarks.
    pub fn with_bookmarks(mut self, bookmarks: Vec<String>) -> Self {
        self.bookmarks = bookmarks;
        self
    }

    /// Set timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.tx_timeout = Some(timeout);
        self
    }

    /// Set access mode.
    pub fn with_mode(mut self, mode: AccessMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set database.
    pub fn with_database(mut self, db: &str) -> Self {
        self.database = Some(db.to_string());
        self
    }

    /// Convert to PackStream structure.
    pub fn to_structure(&self) -> PackStreamStructure {
        let mut extra = HashMap::new();

        if !self.bookmarks.is_empty() {
            let list: Vec<PackStreamValue> = self
                .bookmarks
                .iter()
                .map(|s| PackStreamValue::String(s.clone()))
                .collect();
            extra.insert("bookmarks".to_string(), PackStreamValue::List(list));
        }

        if let Some(timeout) = self.tx_timeout {
            extra.insert(
                "tx_timeout".to_string(),
                PackStreamValue::Integer(timeout.as_millis() as i64),
            );
        }

        if self.mode == AccessMode::Read {
            extra.insert("mode".to_string(), PackStreamValue::String("r".to_string()));
        }

        if let Some(ref db) = self.database {
            extra.insert("db".to_string(), PackStreamValue::String(db.clone()));
        }

        if !self.tx_metadata.is_empty() {
            extra.insert(
                "tx_metadata".to_string(),
                PackStreamValue::Map(self.tx_metadata.clone()),
            );
        }

        PackStreamStructure::new(tag::BEGIN, vec![PackStreamValue::Map(extra)])
    }

    /// Parse from PackStream structure.
    pub fn from_structure(s: &PackStreamStructure) -> Result<Self, PackStreamError> {
        if s.tag != tag::BEGIN {
            return Err(PackStreamError::InvalidStructure(
                "Expected BEGIN tag".to_string(),
            ));
        }

        let extra = if !s.fields.is_empty() {
            s.fields[0].as_map().cloned().unwrap_or_default()
        } else {
            HashMap::new()
        };

        let bookmarks = extra
            .get("bookmarks")
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

        let tx_timeout = extra
            .get("tx_timeout")
            .and_then(|v| v.as_int())
            .map(|ms| Duration::from_millis(ms as u64));

        let mode = extra
            .get("mode")
            .and_then(|v| v.as_str())
            .map(AccessMode::parse)
            .unwrap_or_default();

        let database = extra
            .get("db")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let tx_metadata = extra
            .get("tx_metadata")
            .and_then(|v| v.as_map())
            .cloned()
            .unwrap_or_default();

        Ok(Self {
            bookmarks,
            tx_timeout,
            mode,
            database,
            tx_metadata,
        })
    }
}

impl Default for BeginMessage {
    fn default() -> Self {
        Self::new()
    }
}

/// ROUTE message - Get routing information (Bolt 4.3+).
#[derive(Debug, Clone)]
pub struct RouteMessage {
    /// Routing context
    pub routing: HashMap<String, PackStreamValue>,
    /// Bookmarks
    pub bookmarks: Vec<String>,
    /// Database name
    pub database: Option<String>,
    /// Impersonated user
    pub imp_user: Option<String>,
}

impl RouteMessage {
    /// Create a new ROUTE message.
    pub fn new() -> Self {
        Self {
            routing: HashMap::new(),
            bookmarks: Vec::new(),
            database: None,
            imp_user: None,
        }
    }

    /// Set routing context.
    pub fn with_routing(mut self, routing: HashMap<String, PackStreamValue>) -> Self {
        self.routing = routing;
        self
    }

    /// Set database.
    pub fn with_database(mut self, db: &str) -> Self {
        self.database = Some(db.to_string());
        self
    }

    /// Convert to PackStream structure.
    pub fn to_structure(&self) -> PackStreamStructure {
        let bookmarks: Vec<PackStreamValue> = self
            .bookmarks
            .iter()
            .map(|s| PackStreamValue::String(s.clone()))
            .collect();

        let db = self
            .database
            .as_ref()
            .map(|s| PackStreamValue::String(s.clone()))
            .unwrap_or(PackStreamValue::Null);

        PackStreamStructure::new(
            tag::ROUTE,
            vec![
                PackStreamValue::Map(self.routing.clone()),
                PackStreamValue::List(bookmarks),
                db,
            ],
        )
    }

    /// Parse from PackStream structure.
    pub fn from_structure(s: &PackStreamStructure) -> Result<Self, PackStreamError> {
        if s.tag != tag::ROUTE {
            return Err(PackStreamError::InvalidStructure(
                "Expected ROUTE tag".to_string(),
            ));
        }

        let routing = s
            .fields
            .first()
            .and_then(|v| v.as_map())
            .cloned()
            .unwrap_or_default();

        let bookmarks = s
            .fields
            .get(1)
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

        let database = s
            .fields
            .get(2)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(Self {
            routing,
            bookmarks,
            database,
            imp_user: None,
        })
    }
}

impl Default for RouteMessage {
    fn default() -> Self {
        Self::new()
    }
}

/// LOGON message - Re-authenticate (Bolt 5.1+).
#[derive(Debug, Clone)]
pub struct LogonMessage {
    /// Authentication token
    pub auth: AuthToken,
}

impl LogonMessage {
    /// Create a new LOGON message.
    pub fn new(auth: AuthToken) -> Self {
        Self { auth }
    }

    /// Convert to PackStream structure.
    pub fn to_structure(&self) -> PackStreamStructure {
        PackStreamStructure::new(tag::LOGON, vec![PackStreamValue::Map(self.auth.to_map())])
    }

    /// Parse from PackStream structure.
    pub fn from_structure(s: &PackStreamStructure) -> Result<Self, PackStreamError> {
        if s.tag != tag::LOGON {
            return Err(PackStreamError::InvalidStructure(
                "Expected LOGON tag".to_string(),
            ));
        }

        let auth_map = s.fields.first().and_then(|v| v.as_map()).ok_or_else(|| {
            PackStreamError::InvalidStructure("LOGON requires auth map".to_string())
        })?;

        let auth = AuthToken::from_map(auth_map).ok_or_else(|| {
            PackStreamError::InvalidStructure("Invalid auth in LOGON".to_string())
        })?;

        Ok(Self { auth })
    }
}
