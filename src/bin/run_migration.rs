//! Simple migration runner for dry testing engine
//! 
//! Usage: cargo run --bin run_migration -- <migration_file>
//! Example: cargo run --bin run_migration -- ../axum/migrations/022_replace_event_id_index_with_constraint.sql

use std::env;
use std::fs;
use std::path::PathBuf;
use sqlx::PgPool;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env file
    dotenv::dotenv().ok();
    
    // Get DATABASE_URL from environment
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file");
    
    // Get migration file path from command line args
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo run --bin run_migration -- <migration_file>");
        eprintln!("Example: cargo run --bin run_migration -- ../axum/migrations/022_replace_event_id_index_with_constraint.sql");
        std::process::exit(1);
    }
    
    let migration_file = PathBuf::from(&args[1]);
    
    if !migration_file.exists() {
        eprintln!("Error: Migration file not found: {}", migration_file.display());
        std::process::exit(1);
    }
    
    println!("🔧 Connecting to database...");
    let pool = PgPool::connect(&database_url).await?;
    println!("✅ Connected to database");
    
    println!("📄 Reading migration file: {}", migration_file.display());
    let sql = fs::read_to_string(&migration_file)?;
    
    println!("🚀 Executing migration...");
    
    // Split SQL into statements and execute each one
    // Handle DO blocks properly (they contain semicolons)
    let statements = split_sql_statements(&sql);
    
    for (idx, statement) in statements.iter().enumerate() {
        let trimmed = statement.trim();
        if trimmed.is_empty() {
            continue;
        }
        
        match sqlx::query(trimmed).execute(&pool).await {
            Ok(_) => {
                println!("  ✅ Statement {} executed successfully", idx + 1);
            }
            Err(e) => {
                let error_msg = e.to_string().to_lowercase();
                // Some errors are expected (e.g., already exists)
                if error_msg.contains("already exists") 
                    || error_msg.contains("duplicate")
                    || (error_msg.contains("does not exist") && error_msg.contains("index")) {
                    println!("  ⚠️  Statement {}: {} (expected, continuing)", idx + 1, e);
                    continue;
                } else {
                    eprintln!("  ❌ Error in statement {}: {}", idx + 1, e);
                    eprintln!("  Failed statement: {}", trimmed.chars().take(200).collect::<String>());
                    return Err(e.into());
                }
            }
        }
    }
    
    println!("✅ Migration completed successfully!");
    Ok(())
}

/// Split SQL into individual statements
/// 
/// Handles:
/// - Multiple statements separated by semicolons
/// - Comments (-- and /* */)
/// - String literals (single and double quotes)
/// - DO blocks (which contain semicolons inside $$ delimiters)
fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current_statement = String::new();
    let mut in_string = false;
    let mut string_char = '\0';
    let mut in_comment = false;
    let mut in_do_block = false;
    let mut do_delimiter = String::new();
    let mut do_depth = 0;
    
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0;
    
    while i < chars.len() {
        let ch = chars[i];
        let next_ch = if i + 1 < chars.len() { Some(chars[i + 1]) } else { None };
        
        // Handle DO blocks ($$ ... $$)
        if !in_string && !in_comment {
            if ch == '$' && next_ch == Some('$') {
                if !in_do_block {
                    in_do_block = true;
                    do_delimiter = String::from("$$");
                    do_depth = 1;
                    current_statement.push(ch);
                    current_statement.push(next_ch.unwrap());
                    i += 2;
                    continue;
                } else if do_delimiter == "$$" {
                    do_depth -= 1;
                    if do_depth == 0 {
                        in_do_block = false;
                    }
                    current_statement.push(ch);
                    current_statement.push(next_ch.unwrap());
                    i += 2;
                    continue;
                }
            }
        }
        
        if in_do_block {
            current_statement.push(ch);
            i += 1;
            continue;
        }
        
        // Handle comments
        if !in_string {
            if ch == '-' && next_ch == Some('-') {
                // Single-line comment
                while i < chars.len() && chars[i] != '\n' {
                    current_statement.push(chars[i]);
                    i += 1;
                }
                continue;
            }
            if ch == '/' && next_ch == Some('*') {
                // Multi-line comment
                in_comment = true;
                current_statement.push(ch);
                current_statement.push(next_ch.unwrap());
                i += 2;
                continue;
            }
            if in_comment {
                if ch == '*' && next_ch == Some('/') {
                    in_comment = false;
                    current_statement.push(ch);
                    current_statement.push(next_ch.unwrap());
                    i += 2;
                    continue;
                }
                current_statement.push(ch);
                i += 1;
                continue;
            }
        }
        
        // Handle string literals
        if !in_comment {
            if (ch == '\'' || ch == '"') && !in_string {
                in_string = true;
                string_char = ch;
                current_statement.push(ch);
                i += 1;
                continue;
            }
            if in_string && ch == string_char {
                // Check for escaped quote
                if i + 1 < chars.len() && chars[i + 1] == string_char {
                    current_statement.push(ch);
                    current_statement.push(ch);
                    i += 2;
                    continue;
                }
                in_string = false;
                current_statement.push(ch);
                i += 1;
                continue;
            }
        }
        
        // Handle statement separator
        if !in_string && !in_comment && ch == ';' {
            let trimmed = current_statement.trim();
            if !trimmed.is_empty() {
                statements.push(trimmed.to_string());
            }
            current_statement.clear();
            i += 1;
            continue;
        }
        
        current_statement.push(ch);
        i += 1;
    }
    
    // Add final statement if any
    let trimmed = current_statement.trim();
    if !trimmed.is_empty() {
        statements.push(trimmed.to_string());
    }
    
    statements
}

