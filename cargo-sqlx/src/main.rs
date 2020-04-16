use async_trait::async_trait;
use std::env;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use url::Url;

use dotenv::dotenv;

use sqlx::postgres::PgRow;
use sqlx::Connect;
use sqlx::Executor;
use sqlx::PgConnection;
use sqlx::PgPool;
use sqlx::Row;

use structopt::StructOpt;

use anyhow::{anyhow, Context, Result};

const MIGRATION_FOLDER: &'static str = "migrations";

/// Sqlx commandline tool
#[derive(StructOpt, Debug)]
#[structopt(name = "Sqlx")]
enum Opt {
    Migrate(MigrationCommand),

    #[structopt(alias = "db")]
    Database(DatabaseCommand),
}

/// Adds and runs migrations
#[derive(StructOpt, Debug)]
#[structopt(name = "Sqlx migrator")]
enum MigrationCommand {
    /// Add new migration with name <timestamp>_<migration_name>.sql
    Add { name: String },

    /// Run all migrations
    Run,
}

/// Create or drops database depending on your connection string. Alias: db
#[derive(StructOpt, Debug)]
#[structopt(name = "Sqlx migrator")]
enum DatabaseCommand {
    /// Create database in url
    Create,

    /// Drop database in url
    Drop,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let db_url_raw = env::var("DATABASE_URL").context("Failed to find 'DATABASE_URL'")?;

    let db_url = Url::parse(&db_url_raw)?;

    // This code is taken from: https://github.com/launchbadge/sqlx/blob/master/sqlx-macros/src/lib.rs#L63
    match db_url.scheme() {
        #[cfg(feature = "sqlite")]
        "sqlite" => run_command(&Sqlite { db_url: &db_url_raw }).await?,
        #[cfg(not(feature = "sqlite"))]
        "sqlite" => return Err(anyhow!("Not implemented. DATABASE_URL {} has the scheme of a SQLite database but the `sqlite` feature of sqlx was not enabled",
                            db_url)),

        #[cfg(feature = "postgres")]
        "postgresql" | "postgres" => run_command(&Postgres { db_url: &db_url_raw }).await?,
        #[cfg(not(feature = "postgres"))]
        "postgresql" | "postgres" => Err(anyhow!("DATABASE_URL {} has the scheme of a Postgres database but the `postgres` feature of sqlx was not enabled",
                db_url)),

        #[cfg(feature = "mysql")]
        "mysql" | "mariadb" => return Err(anyhow!("Not implemented")),
        #[cfg(not(feature = "mysql"))]
        "mysql" | "mariadb" => return Err(anyhow!(
            "DATABASE_URL {} has the scheme of a MySQL/MariaDB database but the `mysql` feature of sqlx was not enabled",
             db_url
        )),

        scheme => return Err(anyhow!("unexpected scheme {:?} in DATABASE_URL {}", scheme, db_url)),
    }    

    println!("All done!");
    Ok(())
}

async fn run_command(db_creator: &dyn DatabaseCreator) -> Result<()> {
    let opt = Opt::from_args();

    match opt {
        Opt::Migrate(command) => match command {
            MigrationCommand::Add { name } => add_migration_file(&name)?,
            MigrationCommand::Run => run_migrations().await?,
        },
        Opt::Database(command) => match command {
            DatabaseCommand::Create => run_create_database(db_creator).await?,
            DatabaseCommand::Drop => run_drop_database(db_creator).await?,
        },
    };

    Ok(())
}

async fn run_create_database(db_creator: &dyn DatabaseCreator) -> Result<()> {
    if !db_creator.can_create_database() {
        return Err(anyhow!(
            "Database drop is not implemented for {}",
            db_creator.database_type()
        ));
    }

    let db_name = db_creator.get_database_name()?;
    let db_exists = db_creator.check_if_database_exists(&db_name).await?;

    if !db_exists {
        println!("Creating database: {}", db_name);
        Ok(db_creator.create_database(&db_name).await?)
    } else {
        println!("Database already exists, aborting");
        Ok(())
    }
}

async fn run_drop_database(db_creator: &dyn DatabaseCreator) -> Result<()> {
    if !db_creator.can_drop_database() {
        return Err(anyhow!(
            "Database drop is not implemented for {}",
            db_creator.database_type()
        ));
    }

    let db_name = db_creator.get_database_name()?;
    let db_exists = db_creator.check_if_database_exists(&db_name).await?;

    if db_exists {
        println!("Dropping database: {}", db_name);
        Ok(db_creator.drop_database(&db_name).await?)
    } else {
        println!("Database does not exists, aborting");
        Ok(())
    }
}

fn add_migration_file(name: &str) -> Result<()> {
    use chrono::prelude::*;
    use std::path::PathBuf;

    fs::create_dir_all(MIGRATION_FOLDER).context("Unable to create migrations directory")?;

    let dt = Utc::now();
    let mut file_name = dt.format("%Y-%m-%d_%H-%M-%S").to_string();
    file_name.push_str("_");
    file_name.push_str(name);
    file_name.push_str(".sql");

    let mut path = PathBuf::new();
    path.push(MIGRATION_FOLDER);
    path.push(&file_name);

    let mut file = File::create(path).context("Failed to create file")?;
    file.write_all(b"-- Add migration script here")
        .context("Could not write to file")?;

    println!("Created migration: '{}'", file_name);
    Ok(())
}

pub struct Migration {
    pub name: String,
    pub sql: String,
}

fn load_migrations() -> Result<Vec<Migration>> {
    let entries = fs::read_dir(&MIGRATION_FOLDER).context("Could not find 'migrations' dir")?;

    let mut migrations = Vec::new();

    for e in entries {
        if let Ok(e) = e {
            if let Ok(meta) = e.metadata() {
                if !meta.is_file() {
                    continue;
                }

                if let Some(ext) = e.path().extension() {
                    if ext != "sql" {
                        println!("Wrong ext: {:?}", ext);
                        continue;
                    }
                } else {
                    continue;
                }

                let mut file = File::open(e.path())
                    .with_context(|| format!("Failed to open: '{:?}'", e.file_name()))?;
                let mut contents = String::new();
                file.read_to_string(&mut contents)
                    .with_context(|| format!("Failed to read: '{:?}'", e.file_name()))?;

                migrations.push(Migration {
                    name: e.file_name().to_str().unwrap().to_string(),
                    sql: contents,
                });
            }
        }
    }

    migrations.sort_by(|a, b| a.name.partial_cmp(&b.name).unwrap());

    Ok(migrations)
}

async fn run_migrations() -> Result<()> {
    dotenv().ok();
    let db_url = env::var("DATABASE_URL").context("Failed to find 'DATABASE_URL'")?;

    let mut pool = PgPool::new(&db_url)
        .await
        .context("Failed to connect to pool")?;

    create_migration_table(&mut pool).await?;

    let migrations = load_migrations()?;

    for mig in migrations.iter() {
        let mut tx = pool.begin().await?;

        if check_if_applied(&mut tx, &mig.name).await? {
            println!("Already applied migration: '{}'", mig.name);
            continue;
        }
        println!("Applying migration: '{}'", mig.name);

        tx.execute(&*mig.sql)
            .await
            .with_context(|| format!("Failed to run migration {:?}", &mig.name))?;

        save_applied_migration(&mut tx, &mig.name).await?;

        tx.commit().await.context("Failed")?;
    }

    Ok(())
}

struct DbUrl<'a> {
    base_url: &'a str,
    db_name: &'a str,
}

fn get_base_url<'a>(db_url: &'a str) -> Result<DbUrl> {
    let split: Vec<&str> = db_url.rsplitn(2, '/').collect();

    if split.len() != 2 {
        return Err(anyhow!("Failed to find database name in connection string"));
    }

    let db_name = split[0];
    let base_url = split[1];

    Ok(DbUrl { base_url, db_name })
}

async fn create_migration_table(mut pool: &PgPool) -> Result<()> {
    pool.execute(
        r#"
CREATE TABLE IF NOT EXISTS __migrations (
    migration VARCHAR (255) PRIMARY KEY,
    created TIMESTAMP NOT NULL DEFAULT current_timestamp
);
    "#,
    )
    .await
    .context("Failed to create migration table")?;

    Ok(())
}

async fn check_if_applied(connection: &mut PgConnection, migration: &str) -> Result<bool> {
    let result = sqlx::query(
        "select exists(select migration from __migrations where migration = $1) as exists",
    )
    .bind(migration.to_string())
    .try_map(|row: PgRow| row.try_get("exists"))
    .fetch_one(connection)
    .await
    .context("Failed to check migration table")?;

    Ok(result)
}

async fn save_applied_migration(pool: &mut PgConnection, migration: &str) -> Result<()> {
    sqlx::query("insert into __migrations (migration) values ($1)")
        .bind(migration.to_string())
        .execute(pool)
        .await
        .context("Failed to insert migration")?;

    Ok(())
}

pub struct Postgres<'a> {
    pub db_url: &'a str,
}

pub struct Sqlite<'a> {
    pub db_url: &'a str,
}

#[async_trait]
pub trait DatabaseCreator {
    fn database_type(&self) -> String;

    fn get_database_name(&self) -> Result<String>;

    fn can_migrate_database(&self) -> bool;
    fn can_create_database(&self) -> bool;
    fn can_drop_database(&self) -> bool;

    async fn check_if_database_exists(&self, db_name: &str) -> Result<bool>;
    async fn create_database(&self, db_name: &str) -> Result<()>;
    async fn drop_database(&self, db_name: &str) -> Result<()>;
}

#[async_trait]
impl DatabaseCreator for Postgres<'_> {
    fn database_type(&self) -> String {
        "Postgres".to_string()
    }

    fn can_migrate_database(&self) -> bool {
        true
    }

    fn can_create_database(&self) -> bool {
        true
    }

    fn can_drop_database(&self) -> bool {
        true
    }

    fn get_database_name(&self) -> Result<String> {
        let db_url = get_base_url(self.db_url)?;
        Ok(db_url.db_name.to_string())
    }

    async fn check_if_database_exists(&self, db_name: &str) -> Result<bool> {
        let db_url = get_base_url(self.db_url)?;

        let base_url = db_url.base_url;

        let mut conn = PgConnection::connect(base_url).await?;

        let result: bool =
            sqlx::query("select exists(SELECT 1 from pg_database WHERE datname = $1) as exists")
                .bind(db_name)
                .try_map(|row: PgRow| row.try_get("exists"))
                .fetch_one(&mut conn)
                .await
                .context("Failed to check if database exists")?;

        Ok(result)
    }

    async fn create_database(&self, db_name: &str) -> Result<()> {
        let db_url = get_base_url(self.db_url)?;

        let base_url = db_url.base_url;

        let mut conn = PgConnection::connect(base_url).await?;

        sqlx::query(&format!("CREATE DATABASE {}", db_name))
            .execute(&mut conn)
            .await
            .with_context(|| format!("Failed to create database: {}", db_name))?;

        Ok(())
    }

    async fn drop_database(&self, db_name: &str) -> Result<()> {
        let db_url = get_base_url(self.db_url)?;

        let base_url = db_url.base_url;

        let mut conn = PgConnection::connect(base_url).await?;

        sqlx::query(&format!("DROP DATABASE {}", db_name))
            .execute(&mut conn)
            .await
            .with_context(|| format!("Failed to create database: {}", db_name))?;

        Ok(())
    }
}


#[async_trait]
impl DatabaseCreator for Sqlite<'_> {
    fn database_type(&self) -> String {
        "Postgres".to_string()
    }

    fn can_migrate_database(&self) -> bool {
        true
    }

    fn can_create_database(&self) -> bool {
        true
    }

    fn can_drop_database(&self) -> bool {
        true
    }

    fn get_database_name(&self) -> Result<String> {
        let db_url = get_base_url(self.db_url)?;
        Ok(db_url.db_name.to_string())
    }

    async fn check_if_database_exists(&self, db_name: &str) -> Result<bool> {
        let db_url = get_base_url(self.db_url)?;

        let base_url = db_url.base_url;

        let mut conn = PgConnection::connect(base_url).await?;

        let result: bool =
            sqlx::query("select exists(SELECT 1 from pg_database WHERE datname = $1) as exists")
                .bind(db_name)
                .try_map(|row: PgRow| row.try_get("exists"))
                .fetch_one(&mut conn)
                .await
                .context("Failed to check if database exists")?;

        Ok(result)
    }

    async fn create_database(&self, db_name: &str) -> Result<()> {
        let db_url = get_base_url(self.db_url)?;

        let base_url = db_url.base_url;

        let mut conn = PgConnection::connect(base_url).await?;

        sqlx::query(&format!("CREATE DATABASE {}", db_name))
            .execute(&mut conn)
            .await
            .with_context(|| format!("Failed to create database: {}", db_name))?;

        Ok(())
    }

    async fn drop_database(&self, db_name: &str) -> Result<()> {
        let db_url = get_base_url(self.db_url)?;

        let base_url = db_url.base_url;

        let mut conn = PgConnection::connect(base_url).await?;

        sqlx::query(&format!("DROP DATABASE {}", db_name))
            .execute(&mut conn)
            .await
            .with_context(|| format!("Failed to create database: {}", db_name))?;

        Ok(())
    }
}
