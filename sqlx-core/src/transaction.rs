use std::borrow::Cow;
use std::fmt::{self, Debug, Formatter};
use std::ops::{Deref, DerefMut};

use futures_core::future::BoxFuture;
use futures_util::{future, FutureExt};

use crate::connection::Connection;
use crate::database::Database;
use crate::error::Error;
use crate::pool::MaybePooled;

/// Generic management of database transactions.
///
/// This trait should not be used, except when implementing [`Connection`].
#[doc(hidden)]
pub trait TransactionManager {
    type Database: Database;

    /// Begin a new transaction or establish a savepoint within the active transaction.
    fn begin(
        conn: &mut <Self::Database as Database>::Connection,
        depth: usize,
    ) -> BoxFuture<'_, Result<(), Error>>;

    /// Commit the active transaction or release the most recent savepoint.
    fn commit(
        conn: &mut <Self::Database as Database>::Connection,
        depth: usize,
    ) -> BoxFuture<'_, Result<(), Error>>;

    /// Abort the active transaction or restore from the most recent savepoint.
    fn rollback(
        conn: &mut <Self::Database as Database>::Connection,
        depth: usize,
    ) -> BoxFuture<'_, Result<(), Error>>;

    /// Starts to abort the active transaction or restore from the most recent snapshot.
    fn start_rollback(conn: &mut <Self::Database as Database>::Connection, depth: usize);
}

/// An in-progress database transaction or savepoint.
///
/// A transaction starts with a call to [`Pool::begin`] or [`Connection::begin`].
///
/// A transaction should end with a call to [`commit`] or [`rollback`]. If neither are called
/// before the transaction goes out-of-scope, [`rollback`] is called. In other
/// words, [`rollback`] is called on `drop` if the transaction is still in-progress.
///
/// A savepoint is a special mark inside a transaction that allows all commands that are
/// executed after it was established to be rolled back, restoring the transaction state to
/// what it was at the time of the savepoint.
///
/// [`Connection::begin`]: struct.Connection.html#method.begin
/// [`Pool::begin`]: struct.Pool.html#method.begin
/// [`commit`]: #method.commit
/// [`rollback`]: #method.rollback
pub struct Transaction<'c, DB>
where
    DB: Database,
{
    connection: MaybePooled<'c, DB>,

    // the depth of ~this~ transaction, depth directly equates to how many times [`begin()`]
    // was called without a corresponding [`commit()`] or [`rollback()`]
    depth: usize,

    open: bool,
}

impl<'c, DB> Transaction<'c, DB>
where
    DB: Database,
{
    pub(crate) fn begin(
        conn: impl Into<MaybePooled<'c, DB>>,
    ) -> BoxFuture<'c, Result<Self, Error>> {
        let mut conn = conn.into();

        Box::pin(async move {
            let depth = conn.transaction_depth();

            DB::TransactionManager::begin(conn.get_mut(), depth).await?;

            Ok(Self {
                depth: depth + 1,
                connection: conn,
                open: true,
            })
        })
    }

    /// Commits this transaction or savepoint.
    pub async fn commit(mut self) -> Result<(), Error> {
        DB::TransactionManager::commit(self.connection.get_mut(), self.depth).await?;
        self.open = false;
        Ok(())
    }

    /// Aborts this transaction or savepoint.
    pub async fn rollback(mut self) -> Result<(), Error> {
        DB::TransactionManager::rollback(self.connection.get_mut(), self.depth).await?;
        self.open = false;
        Ok(())
    }
}

// NOTE: required due to lack of lazy normalization
#[allow(unused_macros)]
macro_rules! impl_executor_for_transaction {
    ($DB:ident, $Row:ident) => {
        impl<'c, 't> crate::executor::Executor<'t>
            for &'t mut crate::transaction::Transaction<'c, $DB>
        {
            type Database = $DB;

            fn fetch_many<'e, 'q: 'e, E: 'q>(
                self,
                query: E,
            ) -> futures_core::stream::BoxStream<
                'e,
                Result<either::Either<u64, $Row>, crate::error::Error>,
            >
            where
                't: 'e,
                E: crate::executor::Execute<'q, Self::Database>,
            {
                crate::connection::Connection::get_mut(self).fetch_many(query)
            }

            fn fetch_optional<'e, 'q: 'e, E: 'q>(
                self,
                query: E,
            ) -> futures_core::future::BoxFuture<'e, Result<Option<$Row>, crate::error::Error>>
            where
                't: 'e,
                E: crate::executor::Execute<'q, Self::Database>,
            {
                crate::connection::Connection::get_mut(self).fetch_optional(query)
            }

            #[doc(hidden)]
            fn describe<'e, 'q: 'e, E: 'q>(
                self,
                query: E,
            ) -> futures_core::future::BoxFuture<
                'e,
                Result<crate::statement::StatementInfo<Self::Database>, crate::error::Error>,
            >
            where
                't: 'e,
                E: crate::executor::Execute<'q, Self::Database>,
            {
                crate::connection::Connection::get_mut(self).describe(query)
            }
        }
    };
}

impl<'c, DB> Debug for Transaction<'c, DB>
where
    DB: Database,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // TODO: Show the full type <..<..<..
        f.debug_struct("Transaction").finish()
    }
}

impl<'c, DB> Deref for Transaction<'c, DB>
where
    DB: Database,
{
    type Target = DB::Connection;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.get_ref()
    }
}

impl<'c, DB> DerefMut for Transaction<'c, DB>
where
    DB: Database,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_mut()
    }
}

impl<'c, DB> Drop for Transaction<'c, DB>
where
    DB: Database,
{
    fn drop(&mut self) {
        if self.open {
            // starts a rollback operation
            // what this does depends on the database but generally this means we queue a rollback
            // operation that will happen on the next asynchronous invocation of the underlying
            // connection (including if the connection is returned to a pool)
            DB::TransactionManager::start_rollback(self.connection.get_mut(), self.depth);
        }
    }
}

#[allow(dead_code)]
pub(crate) fn begin_ansi_transaction_sql(depth: usize) -> Cow<'static, str> {
    if depth == 0 {
        Cow::Borrowed("BEGIN")
    } else {
        Cow::Owned(format!("SAVEPOINT _sqlx_savepoint_{}", depth))
    }
}

#[allow(dead_code)]
pub(crate) fn commit_ansi_transaction_sql(depth: usize) -> Cow<'static, str> {
    if depth == 1 {
        Cow::Borrowed("COMMIT")
    } else {
        Cow::Owned(format!("RELEASE SAVEPOINT _sqlx_savepoint_{}", depth - 1))
    }
}

#[allow(dead_code)]
pub(crate) fn rollback_ansi_transaction_sql(depth: usize) -> Cow<'static, str> {
    if depth == 1 {
        Cow::Borrowed("ROLLBACK")
    } else {
        Cow::Owned(format!(
            "ROLLBACK TO SAVEPOINT _sqlx_savepoint_{}",
            depth - 1
        ))
    }
}
