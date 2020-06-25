//! Microsoft SQL (MSSQL) database driver.

mod arguments;
mod connection;
mod database;
mod error;
mod io;
mod options;
mod protocol;
mod row;
mod transaction;
mod type_info;
pub mod types;
mod value;

pub use arguments::MssqlArguments;
pub use connection::MssqlConnection;
pub use database::Mssql;
pub use error::MssqlDatabaseError;
pub use options::MssqlConnectOptions;
pub use row::MssqlRow;
pub use transaction::MssqlTransactionManager;
pub use type_info::MssqlTypeInfo;
pub use value::{MssqlValue, MssqlValueRef};

/// An alias for [`Pool`][crate::pool::Pool], specialized for MySQL.
pub type MssqlPool = crate::pool::Pool<Mssql>;

// NOTE: required due to the lack of lazy normalization
impl_into_arguments_for_arguments!(MssqlArguments);
impl_executor_for_pool_connection!(Mssql, MssqlConnection, MssqlRow);
impl_executor_for_transaction!(Mssql, MssqlRow);

// FIXME: RPC NULL parameter values / results
// FIXME: RPC Empty String parameter values
