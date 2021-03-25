use std::path::{Path, PathBuf};

use super::{default, PgConnectOptions};

impl PgConnectOptions {
    /// Returns the hostname of the database server.
    #[must_use]
    pub fn get_host(&self) -> &str {
        self.address.as_ref().left().map_or(default::HOST, |(host, _)| &**host)
    }

    /// Returns the TCP port number of the database server.
    #[must_use]
    pub fn get_port(&self) -> u16 {
        self.address.as_ref().left().map_or(default::PORT, |(_, port)| *port)
    }

    /// Returns the path to the Unix domain socket, if one is configured.
    #[must_use]
    pub fn get_socket(&self) -> Option<&Path> {
        self.address.as_ref().right().map(PathBuf::as_path)
    }

    /// Returns the default database name.
    #[must_use]
    pub fn get_database(&self) -> Option<&str> {
        self.database.as_deref()
    }

    /// Returns the username to be used for authentication.
    #[must_use]
    pub fn get_username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    /// Returns the password to be used for authentication.
    #[must_use]
    pub fn get_password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    /// Returns the application name for the connection.
    #[must_use]
    pub fn get_application_name(&self) -> Option<&str> {
        self.application_name.as_deref()
    }
}
