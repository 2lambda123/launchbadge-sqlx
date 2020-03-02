#![allow(dead_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(not(any(feature = "runtime-tokio", feature = "runtime-async-std")))]
compile_error!("one of 'runtime-async-std' or 'runtime-tokio' features must be enabled");

#[cfg(all(feature = "runtime-tokio", feature = "runtime-async-std"))]
compile_error!("only one of 'runtime-async-std' or 'runtime-tokio' features must be enabled");

// Modules
pub use sqlx_core::{arguments, describe, error, pool, row, types};

// Types
pub use sqlx_core::{
    Connect, Connection, Cursor, Error, Execute, Executor, FromRow, Pool, Result, Row,
};

pub use sqlx_core::database::{Database, HasCursor, HasRawValue, HasRow};
pub use sqlx_core::query::{self, query, Query};
pub use sqlx_core::query_as::{query_as, QueryAs};

#[cfg(feature = "mysql")]
#[cfg_attr(docsrs, doc(cfg(feature = "mysql")))]
pub use sqlx_core::mysql::{self, MySql, MySqlConnection, MySqlPool};

#[cfg(feature = "postgres")]
#[cfg_attr(docsrs, doc(cfg(feature = "postgres")))]
pub use sqlx_core::postgres::{self, PgConnection, PgPool, Postgres};

#[cfg(feature = "macros")]
#[doc(hidden)]
pub extern crate sqlx_macros;

#[cfg(feature = "macros")]
mod macros;

// macro support
#[cfg(feature = "macros")]
#[doc(hidden)]
pub mod ty_cons;

#[cfg(feature = "macros")]
#[doc(hidden)]
pub mod result_ext;

pub mod encode {
    pub use sqlx_core::encode::{Encode, IsNull};

    #[cfg(feature = "macros")]
    pub use sqlx_macros::Encode;
}

pub mod decode {
    pub use sqlx_core::decode::Decode;

    #[cfg(feature = "macros")]
    pub use sqlx_macros::Decode;
}

pub mod prelude {
    #[cfg(feature = "postgres")]
    pub use super::postgres::PgQueryAs as _;
}
