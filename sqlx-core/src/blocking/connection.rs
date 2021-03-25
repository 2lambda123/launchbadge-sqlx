use super::{Close, Connect, ConnectOptions, Runtime};
use crate::Describe;

/// A unique connection (session) with a specific database.
///
/// For detailed information, refer to the async version of
/// this: [`Connection`][crate::Connection].
///
pub trait Connection<Rt>: crate::Connection<Rt> + Close<Rt> + Connect<Rt>
where
    Rt: Runtime,
    Self::Options: ConnectOptions,
{
    /// Checks if a connection to the database is still valid.
    ///
    /// For detailed information, refer to the async version of
    /// this: [`ping()`][crate::Connection::ping].
    ///
    fn ping(&mut self) -> crate::Result<()>;

    fn describe<'x, 'e, 'q>(
        &'e mut self,
        query: &'q str,
    ) -> crate::Result<Describe<Self::Database>>
    where
        'e: 'x,
        'q: 'x;
}
