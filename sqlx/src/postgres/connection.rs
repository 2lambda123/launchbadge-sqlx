use std::fmt::{self, Debug, Formatter};
use std::ops::{Deref, DerefMut};

#[cfg(feature = "async")]
use futures_util::future::{BoxFuture, FutureExt};
use sqlx_core::{Execute, Executor};

use super::{PgConnectOptions, PgQueryResult, PgRow, Postgres};
#[cfg(feature = "blocking")]
use crate::blocking;
use crate::{Arguments, Close, Connect, Connection, DefaultRuntime, Describe, Runtime};
#[cfg(feature = "async")]
use crate::{Async, Result};

/// A single connection (also known as a session) to a PostgreSQL database server.
#[allow(clippy::module_name_repetitions)]
pub struct PgConnection<Rt: Runtime = DefaultRuntime>(pub(super) sqlx_postgres::PgConnection<Rt>);

#[cfg(feature = "async")]
impl<Rt: Async> PgConnection<Rt> {
    /// Open a new database connection.
    ///
    /// A value of [`PgConnectOptions`] is parsed from the provided
    /// connection `url`.
    ///
    /// ```text
    /// mysql://[[user[:password]@]host][/database][?properties]
    /// ```
    ///
    /// Implemented with [`Connect::connect`][crate::Connect::connect].
    pub async fn connect(url: &str) -> Result<Self> {
        sqlx_postgres::PgConnection::<Rt>::connect(url).await.map(Self)
    }

    /// Open a new database connection with the configured options.
    ///
    /// Implemented with [`Connect::connect_with`][crate::Connect::connect_with].
    pub async fn connect_with(options: &PgConnectOptions<Rt>) -> Result<Self> {
        sqlx_postgres::PgConnection::<Rt>::connect_with(&**options).await.map(Self)
    }

    /// Checks if a connection to the database is still valid.
    ///
    /// Implemented with [`Connection::ping`][crate::Connection::ping].
    pub async fn ping(&mut self) -> Result<()> {
        self.0.ping().await
    }

    // pub async fn execute<'q, 'a, E>(&mut self, query: E) -> Result<PgQueryResult>
    // where
    //     E: Execute<'q, 'a, Postgres>,
    // {
    //     self.0.execute(query).await
    // }
    //
    // pub async fn fetch_all<'q, 'a, E>(&mut self, query: E) -> Result<Vec<PgRow>>
    // where
    //     E: Execute<'q, 'a, Postgres>,
    // {
    //     self.0.fetch_all(query).await
    // }
    //
    // pub async fn fetch_one<'q, 'a, E>(&mut self, query: E) -> Result<PgRow>
    // where
    //     E: Execute<'q, 'a, Postgres>,
    // {
    //     self.0.fetch_one(query).await
    // }
    //
    // pub async fn fetch_optional<'q, 'a, E>(&mut self, query: E) -> Result<Option<PgRow>>
    // where
    //     E: Execute<'q, 'a, Postgres>,
    // {
    //     self.0.fetch_optional(query).await
    // }

    /// Explicitly close this database connection.
    ///
    /// This method is **not required** for safe and consistent operation. However, it is
    /// recommended to call it instead of letting a connection `drop` as MySQL
    /// will be faster at cleaning up resources.
    ///
    /// Implemented with [`Close::close`][crate::Close::close].
    pub async fn close(self) -> Result<()> {
        self.0.close().await
    }
}

impl<Rt: Runtime> Debug for PgConnection<Rt> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl<Rt: Runtime> Close<Rt> for PgConnection<Rt> {
    #[cfg(feature = "async")]
    #[inline]
    fn close(self) -> BoxFuture<'static, Result<()>>
    where
        Rt: Async,
    {
        self.close().boxed()
    }
}

impl<Rt: Runtime> Connect<Rt> for PgConnection<Rt> {
    type Options = PgConnectOptions<Rt>;

    #[cfg(feature = "async")]
    #[inline]
    fn connect_with(options: &Self::Options) -> BoxFuture<'_, Result<Self>>
    where
        Rt: Async,
    {
        Self::connect_with(options).boxed()
    }
}

impl<Rt: Runtime> Connection<Rt> for PgConnection<Rt> {
    type Database = Postgres;

    #[cfg(feature = "async")]
    #[inline]
    fn ping(&mut self) -> BoxFuture<'_, Result<()>>
    where
        Rt: Async,
    {
        self.0.ping()
    }

    #[cfg(feature = "async")]
    #[inline]
    fn describe<'x, 'e, 'q>(
        &'e mut self,
        query: &'q str,
    ) -> BoxFuture<'x, Result<Describe<Postgres>>>
    where
        Rt: Async,
        'e: 'x,
        'q: 'x,
    {
        self.0.describe(query)
    }
}

// impl<Rt: Runtime> Executor<Rt> for PgConnection<Rt> {
//     type Database = Postgres;
//
//     #[cfg(feature = "async")]
//     fn execute<'x, 'e, 'q, 'a, E>(&'e mut self, query: E) -> BoxFuture<'x, Result<PgQueryResult>>
//     where
//         Rt: Async,
//         E: 'x + Execute<'q, 'a, Postgres>,
//         'e: 'x,
//         'q: 'x,
//         'a: 'x,
//     {
//         self.0.execute(query)
//     }
//
//     #[cfg(feature = "async")]
//     fn fetch_all<'x, 'e, 'q, 'a, E>(&'e mut self, query: E) -> BoxFuture<'x, Result<Vec<PgRow>>>
//     where
//         Rt: Async,
//         E: 'x + Execute<'q, 'a, Postgres>,
//         'e: 'x,
//         'q: 'x,
//         'a: 'x,
//     {
//         self.0.fetch_all(query)
//     }
//
//     #[cfg(feature = "async")]
//     fn fetch_optional<'x, 'e, 'q, 'a, E>(
//         &'e mut self,
//         query: E,
//     ) -> BoxFuture<'x, Result<Option<PgRow>>>
//     where
//         Rt: Async,
//         E: 'x + Execute<'q, 'a, Postgres>,
//         'e: 'x,
//         'q: 'x,
//         'a: 'x,
//     {
//         self.0.fetch_optional(query)
//     }
// }

impl<Rt: Runtime> From<sqlx_postgres::PgConnection<Rt>> for PgConnection<Rt> {
    fn from(connection: sqlx_postgres::PgConnection<Rt>) -> Self {
        Self(connection)
    }
}

impl<Rt: Runtime> Deref for PgConnection<Rt> {
    type Target = sqlx_postgres::PgConnection<Rt>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<Rt: Runtime> DerefMut for PgConnection<Rt> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
