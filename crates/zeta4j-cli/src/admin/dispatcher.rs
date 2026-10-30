//! Admin 명령 디스패처 (gRPC 클라이언트 기반)

use std::process::ExitCode;

use rpassword::prompt_password;
use tokio::runtime::Runtime;
use zeta4j_cli_grpc::{AdminClient, GrpcClientConfig};

use super::cli::{ApiKeyCommand, Args, Command, CredentialSessionCommand, SigningKeyCommand};

impl Command {
    /// 명령 실행
    pub fn execute(&self, args: &Args) -> ExitCode {
        // Tokio 런타임 생성
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("Error: Failed to create async runtime: {}", e);
                return ExitCode::FAILURE;
            }
        };

        // gRPC 클라이언트 설정
        let config = GrpcClientConfig::new(&args.address)
            .with_connect_timeout(args.connect_timeout)
            .with_request_timeout(args.request_timeout);

        rt.block_on(self.execute_async(config))
    }

    /// 비동기 명령 실행
    async fn execute_async(&self, config: GrpcClientConfig) -> ExitCode {
        let descriptor = self.command_descriptor();
        log::debug!(
            "dispatching {} to AdminService/{}",
            descriptor.action,
            descriptor.rpc_method
        );
        // gRPC 클라이언트 연결
        let mut client = match AdminClient::connect(&config).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error: Failed to connect to server: {}", e);
                eprintln!("  Address: {}", config.address);
                eprintln!("  Hint: Is the server running? Check with 'z4j server status'");
                return ExitCode::FAILURE;
            }
        };

        // 명령 실행

        match self {
            Command::Status { verbose } => Self::cmd_status(&mut client, *verbose).await,
            Command::Version => Self::cmd_version(&mut client).await,
            Command::ShowConfig => Self::cmd_show_config(&mut client).await,
            Command::ListDatabases => Self::cmd_list_databases(&mut client).await,
            Command::CreateDatabase { name } => Self::cmd_create_database(&mut client, name).await,
            Command::DropDatabase { name, force } => {
                Self::cmd_drop_database(&mut client, name, *force).await
            }
            Command::DropFulltextIndex { database, name } => {
                Self::cmd_drop_fulltext_index(&mut client, database, name).await
            }
            Command::DatabaseStatus { name } => Self::cmd_database_status(&mut client, name).await,
            Command::StartDatabase { name } => Self::cmd_start_database(&mut client, name).await,
            Command::StopDatabase { name } => Self::cmd_stop_database(&mut client, name).await,
            Command::ListUsers => Self::cmd_list_users(&mut client).await,
            Command::CreateUser {
                username,
                password,
                password_env,
            } => {
                let pwd = match resolve_password(password.clone(), password_env.clone()) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        return ExitCode::FAILURE;
                    }
                };
                Self::cmd_create_user(&mut client, username, &pwd).await
            }
            Command::DeleteUser { username } => Self::cmd_delete_user(&mut client, username).await,
            Command::ApiKey { command } => Self::cmd_api_key(&mut client, command).await,
            Command::CredentialSession { command } => {
                Self::cmd_credential_session(&mut client, command).await
            }
            Command::SigningKey { command } => Self::cmd_signing_key(&mut client, command).await,
            Command::SetInitialPassword {
                username,
                password,
                password_env,
                require_change,
            } => {
                let pwd = match resolve_password(password.clone(), password_env.clone()) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        return ExitCode::FAILURE;
                    }
                };
                Self::cmd_set_initial_password(&mut client, username, &pwd, *require_change).await
            }
            Command::Backup { database, output } => {
                Self::cmd_backup(&mut client, database, output).await
            }
            Command::Restore { database, input } => {
                Self::cmd_restore(&mut client, database, input).await
            }
            Command::Import {
                input,
                format,
                data_type,
                label,
                chunk_size_kib,
                delimiter,
                verbose,
            } => {
                Self::cmd_import(
                    &mut client,
                    input,
                    *format,
                    data_type,
                    label.clone(),
                    *chunk_size_kib,
                    *delimiter,
                    *verbose,
                )
                .await
            }
            Command::Export {
                output,
                format,
                data_type,
                label,
                delimiter,
                verbose,
            } => {
                Self::cmd_export(
                    &mut client,
                    output,
                    *format,
                    data_type,
                    label.clone(),
                    *delimiter,
                    *verbose,
                )
                .await
            }
        }
    }

    async fn cmd_api_key(client: &mut AdminClient, command: &ApiKeyCommand) -> ExitCode {
        let result = match command {
            ApiKeyCommand::List => match client.list_api_keys().await {
                Ok(api_keys) => {
                    println!(
                        "{:<38} {:<20} {:<12} {:<8} {:<12} {:>10}",
                        "ID", "NAME", "STATUS", "VERSION", "AUDIENCE", "EXPIRES"
                    );
                    for key in api_keys {
                        println!(
                            "{:<38} {:<20} {:<12} {:<8} {:<12} {:>10}",
                            key.id, key.name, key.status, key.version, key.audience, key.expires_at
                        );
                    }
                    return ExitCode::SUCCESS;
                }
                Err(error) => Err(error),
            },
            ApiKeyCommand::Create {
                name,
                audience,
                scopes,
                expires_in_seconds,
            } => {
                let expires_at = match deadline(*expires_in_seconds) {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("Error: {error}");
                        return ExitCode::FAILURE;
                    }
                };
                client
                    .create_api_key(name, audience, scopes.clone(), expires_at)
                    .await
            }
            ApiKeyCommand::Rotate {
                id,
                overlap_seconds,
                expires_in_seconds,
            } => {
                let overlap_until = match deadline(*overlap_seconds) {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("Error: {error}");
                        return ExitCode::FAILURE;
                    }
                };
                let expires_at = match deadline(*expires_in_seconds) {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("Error: {error}");
                        return ExitCode::FAILURE;
                    }
                };
                client.rotate_api_key(id, overlap_until, expires_at).await
            }
            ApiKeyCommand::Revoke { id } => match client.revoke_api_key(id).await {
                Ok(api_key) => {
                    println!("API key '{}' revoked.", api_key.id);
                    return ExitCode::SUCCESS;
                }
                Err(error) => Err(error),
            },
        };

        match result {
            Ok(response) => {
                let Some(api_key) = response.api_key else {
                    eprintln!("Error: server omitted committed API key metadata");
                    return ExitCode::FAILURE;
                };
                println!("API key '{}' committed.", api_key.id);
                println!("Secret (shown once): {}", response.secret);
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("Error: {error}");
                ExitCode::FAILURE
            }
        }
    }

    async fn cmd_credential_session(
        client: &mut AdminClient,
        command: &CredentialSessionCommand,
    ) -> ExitCode {
        match command {
            CredentialSessionCommand::List => match client.list_credential_sessions().await {
                Ok(sessions) => {
                    println!(
                        "{:<38} {:<38} {:<14} {:<18} {:>10}",
                        "SESSION", "SUBJECT", "KIND", "STATUS", "EXPIRES"
                    );
                    for session in sessions {
                        println!(
                            "{:<38} {:<38} {:<14} {:<18} {:>10}",
                            session.id,
                            session.subject_id,
                            session.credential_kind,
                            session.status,
                            session.absolute_expires_at
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    ExitCode::FAILURE
                }
            },
            CredentialSessionCommand::Revoke { id } => {
                match client.revoke_credential_session(id).await {
                    Ok(response) if response.success => {
                        println!("Credential session '{id}' revoked.");
                        ExitCode::SUCCESS
                    }
                    Ok(response) => {
                        eprintln!("Error: {}", response.message);
                        ExitCode::FAILURE
                    }
                    Err(error) => {
                        eprintln!("Error: {error}");
                        ExitCode::FAILURE
                    }
                }
            }
        }
    }

    async fn cmd_signing_key(client: &mut AdminClient, command: &SigningKeyCommand) -> ExitCode {
        match command {
            SigningKeyCommand::List => match client.list_signing_keys().await {
                Ok(keys) => {
                    println!(
                        "{:>8} {:<32} {:<16} {:>14}",
                        "VERSION", "KEY ID", "STATUS", "VERIFY UNTIL"
                    );
                    for key in keys {
                        println!(
                            "{:>8} {:<32} {:<16} {:>14}",
                            key.version,
                            key.key_id,
                            key.status,
                            key.verify_until
                                .map_or_else(|| "-".to_string(), |value| value.to_string())
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    ExitCode::FAILURE
                }
            },
            SigningKeyCommand::Compromise { key_version } => {
                match client.compromise_signing_key(*key_version).await {
                    Ok(response) if response.success => {
                        println!("Signing key version {key_version} marked compromised.");
                        ExitCode::SUCCESS
                    }
                    Ok(response) => {
                        eprintln!("Error: {}", response.message);
                        ExitCode::FAILURE
                    }
                    Err(error) => {
                        eprintln!("Error: {error}");
                        ExitCode::FAILURE
                    }
                }
            }
        }
    }

    // =========================================================================
    // System Commands
    // =========================================================================

    async fn cmd_status(client: &mut AdminClient, verbose: bool) -> ExitCode {
        match client.get_system_status().await {
            Ok(status) => {
                println!("System Status");
                println!("=============");
                println!("Edition:      {}", status.edition);
                println!("Version:      {}", status.version);
                println!("Status:       {}", status.status);
                println!("Uptime:       {} seconds", status.uptime_seconds);
                println!(
                    "Memory:       {:.1} MB / {:.1} MB",
                    status.memory_used_bytes as f64 / 1024.0 / 1024.0,
                    status.memory_total_bytes as f64 / 1024.0 / 1024.0
                );
                println!("Connections:  {}", status.active_connections);
                println!("Transactions: {}", status.active_transactions);

                if verbose && !status.services.is_empty() {
                    println!("\nServices:");
                    for svc in &status.services {
                        let msg = svc.message.as_deref().unwrap_or("");
                        println!("  - {} [{}] {}", svc.name, svc.status, msg);
                    }
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    async fn cmd_version(client: &mut AdminClient) -> ExitCode {
        match client.get_version().await {
            Ok(v) => {
                println!("Zeta4j {}", v.version);
                println!("Edition:      {}", v.edition);
                println!("Commit:       {}", v.commit_hash);
                println!("Build Date:   {}", v.build_date);
                println!("Rust Version: {}", v.rust_version);
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    async fn cmd_show_config(client: &mut AdminClient) -> ExitCode {
        match client.get_config().await {
            Ok(cfg) => {
                println!("# Configuration ({})", cfg.format);
                println!("{}", cfg.content);
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    // =========================================================================
    // Database Commands
    // =========================================================================

    async fn cmd_list_databases(client: &mut AdminClient) -> ExitCode {
        match client.list_databases().await {
            Ok(databases) => {
                if databases.is_empty() {
                    println!("No databases found.");
                } else {
                    // 동적 컬럼: 풀텍스트 데이터가 있는 경우에만 표시
                    let show_fulltext = databases.iter().any(|db| db.has_fulltext_index);

                    // 헤더 구성
                    let mut header = format!(
                        "{:<20} {:<10} {:>8} {:>8}",
                        "NAME", "STATUS", "NODES", "EDGES"
                    );
                    if show_fulltext {
                        header.push_str(&format!("  {:>8}", "FULLTEXT"));
                    }
                    println!("{}", header);
                    println!("{}", "-".repeat(header.len()));

                    for db in databases {
                        let mut line = format!(
                            "{:<20} {:<10} {:>8} {:>8}",
                            db.name, db.status, db.node_count, db.edge_count
                        );
                        if show_fulltext {
                            let ft_str = if db.has_fulltext_index {
                                "yes".to_string()
                            } else {
                                "-".to_string()
                            };
                            line.push_str(&format!("  {:>8}", ft_str));
                        }
                        println!("{}", line);
                    }
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    async fn cmd_create_database(client: &mut AdminClient, name: &str) -> ExitCode {
        match client.create_database(name).await {
            Ok(resp) => {
                if resp.success {
                    println!("Database '{}' created successfully.", name);
                    ExitCode::SUCCESS
                } else {
                    eprintln!("Error: {}", resp.message);
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    async fn cmd_drop_database(client: &mut AdminClient, name: &str, force: bool) -> ExitCode {
        match client.drop_database(name, force).await {
            Ok(resp) => {
                if resp.success {
                    println!("Database '{}' dropped successfully.", name);
                    ExitCode::SUCCESS
                } else {
                    eprintln!("Error: {}", resp.message);
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    async fn cmd_drop_fulltext_index(
        client: &mut AdminClient,
        database: &str,
        name: &str,
    ) -> ExitCode {
        match client.drop_fulltext_index(database, name).await {
            Ok(resp) => {
                if resp.success {
                    println!("Fulltext index '{}' dropped successfully.", name);
                    ExitCode::SUCCESS
                } else {
                    eprintln!("Error: {}", resp.message);
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    async fn cmd_database_status(client: &mut AdminClient, name: &str) -> ExitCode {
        match client.get_database_status(name).await {
            Ok(resp) => {
                if let Some(db) = resp.database {
                    println!("Database: {}", db.name);
                    println!("Status:   {}", db.status);
                    println!("Nodes:    {}", db.node_count);
                    println!("Edges:    {}", db.edge_count);
                    println!("Size:     {:.2} MB", db.size_bytes as f64 / 1024.0 / 1024.0);
                    println!("Created:  {}", db.created_at);
                    println!("Updated:  {}", db.updated_at);
                    if db.has_fulltext_index {
                        println!("Fulltext: yes");
                    }
                    println!();
                    println!("Storage:          {}", resp.storage_path);
                    println!(
                        "WAL Size:         {:.2} MB",
                        resp.wal_size as f64 / 1024.0 / 1024.0
                    );
                    println!("Active Txns:      {}", resp.active_transactions);
                    println!(
                        "Memory Usage:     {:.2} MB",
                        resp.memory_usage_bytes as f64 / 1024.0 / 1024.0
                    );
                } else {
                    eprintln!("Database '{}' not found.", name);
                    return ExitCode::FAILURE;
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    async fn cmd_start_database(client: &mut AdminClient, name: &str) -> ExitCode {
        match client.start_database(name).await {
            Ok(resp) => {
                if resp.success {
                    println!("Database '{}' started.", name);
                    ExitCode::SUCCESS
                } else {
                    eprintln!("Error: {}", resp.message);
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    async fn cmd_stop_database(client: &mut AdminClient, name: &str) -> ExitCode {
        match client.stop_database(name).await {
            Ok(resp) => {
                if resp.success {
                    println!("Database '{}' stopped.", name);
                    ExitCode::SUCCESS
                } else {
                    eprintln!("Error: {}", resp.message);
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    // =========================================================================
    // User Commands
    // =========================================================================

    async fn cmd_list_users(client: &mut AdminClient) -> ExitCode {
        match client.list_users().await {
            Ok(users) => {
                if users.is_empty() {
                    println!("No users found.");
                } else {
                    println!("{:<20} {:<12} {:<20}", "USERNAME", "STATUS", "CREATED");
                    println!("{}", "-".repeat(54));
                    for user in users {
                        println!(
                            "{:<20} {:<12} {:<20}",
                            user.username, user.status, user.created_at
                        );
                    }
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    async fn cmd_create_user(client: &mut AdminClient, username: &str, password: &str) -> ExitCode {
        match client.create_user(username, password).await {
            Ok(resp) => {
                if resp.success {
                    println!("User '{}' created.", username);
                    ExitCode::SUCCESS
                } else {
                    eprintln!("Error: {}", resp.message);
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    async fn cmd_delete_user(client: &mut AdminClient, username: &str) -> ExitCode {
        match client.delete_user(username).await {
            Ok(resp) => {
                if resp.success {
                    println!("User '{}' deleted.", username);
                    ExitCode::SUCCESS
                } else {
                    eprintln!("Error: {}", resp.message);
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    async fn cmd_set_initial_password(
        client: &mut AdminClient,
        username: &str,
        password: &str,
        require_change: bool,
    ) -> ExitCode {
        match client
            .set_initial_password(username, password, require_change)
            .await
        {
            Ok(resp) => {
                if resp.success {
                    println!("Password for '{}' has been set.", username);
                    if require_change {
                        println!("User will be required to change password on first login.");
                    }
                    ExitCode::SUCCESS
                } else {
                    eprintln!("Error: {}", resp.message);
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    // =========================================================================
    // Backup/Restore Commands
    // =========================================================================

    async fn cmd_backup(
        client: &mut AdminClient,
        database: &str,
        output: &std::path::Path,
    ) -> ExitCode {
        use tokio::io::AsyncWriteExt;
        use tokio_stream::StreamExt;

        let mut stream = match client.backup(database).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: {}", e);
                return ExitCode::FAILURE;
            }
        };
        let (partial, mut file) = match create_partial_output(output).await {
            Ok(value) => value,
            Err(error) => {
                eprintln!("Error: {error}");
                return ExitCode::FAILURE;
            }
        };

        println!(
            "Creating protected backup for database '{}' at {}...",
            database,
            output.display()
        );

        let mut completed = false;

        while let Some(progress) = stream.next().await {
            match progress {
                Ok(p) => {
                    // 데이터 청크가 있으면 파일에 쓰기
                    if let Some(chunk) = p.data_chunk
                        && let Err(e) = file.write_all(&chunk).await
                    {
                        eprintln!("Error: Failed to write to file: {}", e);
                        discard_partial_output(&partial).await;
                        return ExitCode::FAILURE;
                    }

                    // 진행률 표시
                    if p.status == "in_progress" {
                        eprint!(
                            "\rProgress: {:.1}% (nodes: {}, edges: {})",
                            p.progress_percent, p.nodes_dumped, p.edges_dumped
                        );
                    }

                    if p.status == zeta4j_common::STATUS_COMPLETED {
                        eprintln!();
                        println!(
                            "Protected backup completed: {} nodes, {} edges",
                            p.nodes_dumped, p.edges_dumped
                        );
                        completed = true;
                        break;
                    }

                    if p.status == zeta4j_common::STATUS_FAILED {
                        eprintln!();
                        eprintln!(
                            "Error: Protected backup failed - {}",
                            p.message.unwrap_or_default()
                        );
                        discard_partial_output(&partial).await;
                        return ExitCode::FAILURE;
                    }
                }
                Err(e) => {
                    eprintln!();
                    eprintln!("Error: Stream error - {}", e);
                    discard_partial_output(&partial).await;
                    return ExitCode::FAILURE;
                }
            }
        }

        if !completed {
            eprintln!("Error: backup stream ended without a completion result");
            discard_partial_output(&partial).await;
            return ExitCode::FAILURE;
        }
        if let Err(error) = publish_partial_output(file, &partial, output).await {
            eprintln!("Error: {error}");
            discard_partial_output(&partial).await;
            return ExitCode::FAILURE;
        }

        ExitCode::SUCCESS
    }

    async fn cmd_restore(
        client: &mut AdminClient,
        database: &str,
        input: &std::path::Path,
    ) -> ExitCode {
        use futures::stream;
        use tokio::io::AsyncReadExt;

        // 입력 파일 열기
        let mut file = match tokio::fs::File::open(input).await {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Error: Failed to open input file: {}", e);
                return ExitCode::FAILURE;
            }
        };

        println!(
            "Restoring protected backup into database '{}' from {}...",
            database,
            input.display()
        );

        // 파일을 청크로 읽어서 스트림 생성
        let chunk_size = 64 * 1024; // 64KB chunks
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; chunk_size];

        loop {
            let n = match file.read(&mut buffer).await {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("Error: Failed to read file: {}", e);
                    return ExitCode::FAILURE;
                }
            };

            if n == 0 {
                break;
            }

            chunks.push(zeta4j_cli_grpc::admin::RestoreChunk {
                database: database.to_string(),
                data: buffer[..n].to_vec(),
                is_last: false,
            });
        }

        // 마지막 청크 표시
        if let Some(last) = chunks.last_mut() {
            last.is_last = true;
        } else {
            // 빈 파일인 경우
            chunks.push(zeta4j_cli_grpc::admin::RestoreChunk {
                database: database.to_string(),
                data: Vec::new(),
                is_last: true,
            });
        }

        let chunk_stream = stream::iter(chunks);
        match client.restore(chunk_stream).await {
            Ok(resp) => {
                if resp.success {
                    println!("Protected restore completed:");
                    println!("  Nodes loaded: {}", resp.nodes_loaded);
                    println!("  Edges loaded: {}", resp.edges_loaded);
                    println!("  Duration: {} ms", resp.duration_ms);
                    ExitCode::SUCCESS
                } else {
                    eprintln!("Error: {}", resp.message);
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    // =========================================================================
    // Import/Export Commands
    // =========================================================================

    #[allow(clippy::too_many_arguments)]
    async fn cmd_import(
        client: &mut AdminClient,
        input: &std::path::Path,
        format: super::cli::IoFormat,
        data_type: &str,
        label: Option<String>,
        chunk_size_kib: usize,
        delimiter: char,
        verbose: bool,
    ) -> ExitCode {
        use futures::stream;
        use tokio::io::AsyncReadExt;

        let format_str = match format {
            super::cli::IoFormat::Csv => "csv",
            super::cli::IoFormat::Json => "json",
            super::cli::IoFormat::Ndjson => "ndjson",
        };

        // 입력 파일 열기
        let mut file = match tokio::fs::File::open(input).await {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Error: Failed to open input file: {}", e);
                return ExitCode::FAILURE;
            }
        };

        if verbose {
            println!("Importing {} from {}...", data_type, input.display());
        }

        // 파일을 청크로 읽기
        let chunk_size = chunk_size_kib.saturating_mul(1024);
        if chunk_size == 0 {
            eprintln!("Error: --chunk-size-kib must be greater than zero");
            return ExitCode::FAILURE;
        }
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; chunk_size];

        loop {
            let n = match file.read(&mut buffer).await {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("Error: Failed to read file: {}", e);
                    return ExitCode::FAILURE;
                }
            };

            if n == 0 {
                break;
            }

            chunks.push(zeta4j_cli_grpc::admin::ImportChunk {
                format: format_str.to_string(),
                data_type: data_type.to_string(),
                label: label.clone(),
                data: buffer[..n].to_vec(),
                is_last: false,
                delimiter: delimiter.to_string(),
            });
        }

        // 마지막 청크 표시
        if let Some(last) = chunks.last_mut() {
            last.is_last = true;
        } else {
            chunks.push(zeta4j_cli_grpc::admin::ImportChunk {
                format: format_str.to_string(),
                data_type: data_type.to_string(),
                label: label.clone(),
                data: Vec::new(),
                is_last: true,
                delimiter: delimiter.to_string(),
            });
        }

        // 임포트 실행
        let chunk_stream = stream::iter(chunks);
        match client.import(chunk_stream).await {
            Ok(resp) => {
                if resp.success {
                    println!("Import completed:");
                    println!("  Records imported: {}", resp.records_imported);
                    println!("  Records skipped:  {}", resp.records_skipped);
                    println!("  Errors:           {}", resp.errors);
                    println!("  Duration:         {} ms", resp.duration_ms);
                    ExitCode::SUCCESS
                } else {
                    eprintln!("Error: {}", resp.message);
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::FAILURE
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn cmd_export(
        client: &mut AdminClient,
        output: &std::path::Path,
        format: super::cli::IoFormat,
        data_type: &str,
        label: Option<String>,
        delimiter: char,
        verbose: bool,
    ) -> ExitCode {
        use tokio::io::AsyncWriteExt;
        use tokio_stream::StreamExt;

        let format_str = match format {
            super::cli::IoFormat::Csv => "csv",
            super::cli::IoFormat::Json => "json",
            super::cli::IoFormat::Ndjson => "ndjson",
        };

        if verbose {
            println!("Exporting {} to {}...", data_type, output.display());
        }

        // 익스포트 스트리밍 시작
        let delimiter_str = delimiter.to_string();
        let mut stream = match client
            .export(
                format_str,
                data_type,
                label.as_deref(),
                Some(&delimiter_str),
            )
            .await
        {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: {}", e);
                return ExitCode::FAILURE;
            }
        };
        let (partial, mut file) = match create_partial_output(output).await {
            Ok(value) => value,
            Err(error) => {
                eprintln!("Error: {error}");
                return ExitCode::FAILURE;
            }
        };

        let mut total_records = 0u64;
        let mut completed = false;

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(c) => {
                    // 데이터를 파일에 쓰기
                    if !c.data.is_empty()
                        && let Err(e) = file.write_all(&c.data).await
                    {
                        eprintln!("Error: Failed to write to file: {}", e);
                        discard_partial_output(&partial).await;
                        return ExitCode::FAILURE;
                    }

                    total_records = c.records_exported;

                    if c.is_last {
                        completed = true;
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("Error: Stream error - {}", e);
                    discard_partial_output(&partial).await;
                    return ExitCode::FAILURE;
                }
            }
        }

        if !completed {
            eprintln!("Error: export stream ended without a completion result");
            discard_partial_output(&partial).await;
            return ExitCode::FAILURE;
        }
        if let Err(error) = publish_partial_output(file, &partial, output).await {
            eprintln!("Error: {error}");
            discard_partial_output(&partial).await;
            return ExitCode::FAILURE;
        }

        println!(
            "Export completed: {} records written to {}",
            total_records,
            output.display()
        );
        ExitCode::SUCCESS
    }

    /// 명령 이름
    pub fn name(&self) -> &'static str {
        match self {
            Command::Backup { .. } => "backup",
            Command::Restore { .. } => "restore",
            Command::SetInitialPassword { .. } => "set-initial-password",
            Command::ShowConfig => "show-config",
            Command::Version => "version",
            Command::ListDatabases => "databases",
            Command::CreateDatabase { .. } => "create-database",
            Command::DropDatabase { .. } => "drop-database",
            Command::DropFulltextIndex { .. } => "drop-fulltext-index",
            Command::DatabaseStatus { .. } => "database-status",
            Command::StartDatabase { .. } => "start-database",
            Command::StopDatabase { .. } => "stop-database",
            Command::Status { .. } => "status",
            Command::ListUsers => "users",
            Command::CreateUser { .. } => "create-user",
            Command::DeleteUser { .. } => "delete-user",
            Command::ApiKey { .. } => "api-key",
            Command::CredentialSession { .. } => "session",
            Command::SigningKey { .. } => "signing-key",
            Command::Import { .. } => "import",
            Command::Export { .. } => "export",
        }
    }
}

async fn create_partial_output(
    output: &std::path::Path,
) -> Result<(std::path::PathBuf, tokio::fs::File), String> {
    let parent = output.parent().unwrap_or_else(|| std::path::Path::new("."));
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("output path has no valid file name: {}", output.display()))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("system clock error: {error}"))?
        .as_nanos();
    let partial = parent.join(format!(".{name}.partial-{}-{nonce}", std::process::id()));
    let file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .await
        .map_err(|error| format!("failed to create {}: {error}", partial.display()))?;
    Ok((partial, file))
}

async fn publish_partial_output(
    mut file: tokio::fs::File,
    partial: &std::path::Path,
    output: &std::path::Path,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    file.flush()
        .await
        .map_err(|error| format!("failed to flush {}: {error}", partial.display()))?;
    file.sync_all()
        .await
        .map_err(|error| format!("failed to sync {}: {error}", partial.display()))?;
    drop(file);
    tokio::fs::rename(partial, output).await.map_err(|error| {
        format!(
            "failed to publish {} as {}: {error}",
            partial.display(),
            output.display()
        )
    })
}

async fn discard_partial_output(partial: &std::path::Path) {
    if let Err(error) = tokio::fs::remove_file(partial).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "Warning: failed to remove partial output {}: {error}",
            partial.display()
        );
    }
}

fn deadline(seconds: u64) -> Result<i64, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("system clock error: {error}"))?
        .as_secs();
    let deadline = now
        .checked_add(seconds)
        .ok_or_else(|| "credential deadline overflowed".to_string())?;
    i64::try_from(deadline).map_err(|_| "credential deadline is out of range".to_string())
}

/// 비밀번호 해석
fn resolve_password(
    password_arg: Option<String>,
    password_env: Option<String>,
) -> Result<String, String> {
    if let Some(pwd) = password_env {
        return Ok(pwd);
    }
    if let Some(pwd) = password_arg {
        eprintln!("Warning: Passing password via CLI is insecure.");
        return Ok(pwd);
    }
    prompt_password("Enter password: ").map_err(|e| format!("Failed to read password: {}", e))
}
