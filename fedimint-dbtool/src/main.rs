#![allow(where_clauses_object_safety)] // https://github.com/dtolnay/async-trait/issues/228
use std::path::PathBuf;

use anyhow::Result;
use bitcoin_hashes::hex::ToHex;
use bytes::Bytes;
use clap::{Parser, Subcommand};
use fedimint_core::db::IDatabase;
use fedimint_logging::TracingSetup;
use futures::StreamExt;

use crate::dump::DatabaseDump;

mod dump;

#[derive(Debug, Clone, Parser)]
struct Options {
    database: String,
    #[command(subcommand)]
    command: DbCommand,
}

/// Tool to inspect and manipulate rocksdb databases. All binary arguments
/// (keys, values) have to be hex encoded.
#[derive(Debug, Clone, Subcommand)]
enum DbCommand {
    /// List all key-value pairs where the key begins with `prefix`
    List {
        #[arg(value_parser = hex_parser)]
        prefix: Bytes,
    },
    /// Write a key-value pair to the database, overwriting the previous value
    /// if present
    Write {
        #[arg(value_parser = hex_parser)]
        key: Bytes,
        #[arg(value_parser = hex_parser)]
        value: Bytes,
    },
    /// Delete a single entry from the database identified by `key`
    Delete {
        #[arg(value_parser = hex_parser)]
        key: Bytes,
    },
    /// Dump a subset of the specified database and serialize the retrieved data
    /// to JSON. Module and prefix are used to specify which subset of the
    /// database to dump. Password is used to decrypt the server's
    /// configuration file. If dumping the client database, the password can
    /// be an arbitrary string.
    Dump {
        cfg_dir: PathBuf,
        #[arg(env = "FM_PASSWORD")]
        password: String,
        #[arg(required = false)]
        modules: Option<String>,
        #[arg(required = false)]
        prefixes: Option<String>,
    },
}

fn hex_parser(hex: &str) -> Result<Bytes> {
    let bytes: Vec<u8> = bitcoin_hashes::hex::FromHex::from_hex(hex)?;
    Ok(bytes.into())
}

fn print_kv(key: &[u8], value: &[u8]) {
    println!("{} {}", key.to_hex(), value.to_hex());
}

#[tokio::main]
async fn main() -> Result<()> {
    TracingSetup::default().init()?;
    let options: Options = Options::parse();

    match options.command {
        DbCommand::List { prefix } => {
            let rocksdb: Box<dyn IDatabase> =
                Box::new(fedimint_rocksdb::RocksDb::open(&options.database).unwrap());
            let mut dbtx = rocksdb.begin_transaction().await;
            let prefix_iter = dbtx
                .raw_find_by_prefix(&prefix)
                .await?
                .collect::<Vec<_>>()
                .await;
            for (key, value) in prefix_iter {
                print_kv(&key, &value);
            }
            dbtx.commit_tx().await.expect("Error committing to RocksDb");
        }
        DbCommand::Write { key, value } => {
            let rocksdb: Box<dyn IDatabase> =
                Box::new(fedimint_rocksdb::RocksDb::open(&options.database).unwrap());
            let mut dbtx = rocksdb.begin_transaction().await;
            dbtx.raw_insert_bytes(&key, value.into())
                .await
                .expect("Error inserting entry into RocksDb");
            dbtx.commit_tx().await.expect("Error committing to RocksDb");
        }
        DbCommand::Delete { key } => {
            let rocksdb: Box<dyn IDatabase> =
                Box::new(fedimint_rocksdb::RocksDb::open(&options.database).unwrap());
            let mut dbtx = rocksdb.begin_transaction().await;
            dbtx.raw_remove_entry(&key)
                .await
                .expect("Error removing entry from RocksDb");
            dbtx.commit_tx().await.expect("Error committing to RocksDb");
        }
        DbCommand::Dump {
            cfg_dir,
            modules,
            prefixes,
            password,
        } => {
            let modules = match modules {
                Some(mods) => mods
                    .split(',')
                    .map(|s| s.to_string().to_lowercase())
                    .collect::<Vec<String>>(),
                None => Vec::new(),
            };

            let prefix_names = match prefixes {
                Some(db_prefixes) => db_prefixes
                    .split(',')
                    .map(|s| s.to_string().to_lowercase())
                    .collect::<Vec<String>>(),
                None => Vec::new(),
            };

            let mut dbdump =
                DatabaseDump::new(cfg_dir, options.database, password, modules, prefix_names);
            dbdump.dump_database().await;
        }
    }

    Ok(())
}
