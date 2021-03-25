use std::borrow::Cow;
use std::str::FromStr;

use percent_encoding::percent_decode_str;
use sqlx_core::Error;
use url::Url;

use crate::PgConnectOptions;

impl FromStr for PgConnectOptions {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url: Url = s.parse().map_err(|error| Error::opt("for database URL", error))?;

        if !matches!(url.scheme(), "postgres" | "postgresql") {
            return Err(Error::opt_msg(format!(
                "unsupported URL scheme {:?} for Postgres",
                url.scheme()
            )));
        }

        let mut options = Self::new();

        if let Some(host) = url.host_str() {
            options.host(percent_decode_str_utf8(host));
        }

        if let Some(port) = url.port() {
            options.port(port);
        }

        let username = url.username();
        if !username.is_empty() {
            options.username(percent_decode_str_utf8(username));
        }

        if let Some(password) = url.password() {
            options.password(percent_decode_str_utf8(password));
        }

        let mut path = url.path();

        if path.starts_with('/') {
            path = &path[1..];
        }

        if !path.is_empty() {
            options.database(path);
        }

        for (key, value) in url.query_pairs() {
            match &*key {
                "host" | "hostaddr" => {
                    options.host(value);
                }

                "port" => {
                    options.port(value.parse().map_err(|err| Error::opt("for port", err))?);
                }

                "user" | "username" => {
                    options.username(value);
                }

                "password" => {
                    options.password(value);
                }

                "ssl-mode" | "sslmode" | "sslMode" | "tls" => {
                    todo!()
                }

                "socket" => {
                    options.socket(&*value);
                }

                "application_name" => {
                    options.application_name(&*value);
                }

                _ => {
                    // ignore unknown connection parameters
                    // fixme: should we error or warn here?
                }
            }
        }

        Ok(options)
    }
}

fn percent_decode_str_utf8(value: &str) -> Cow<'_, str> {
    percent_decode_str(value).decode_utf8_lossy()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::PgConnectOptions;

    #[test]
    fn parse() {
        let url = "postgresql://user:password@hostname:8915/database?application_name=sqlx";
        let options: PgConnectOptions = url.parse().unwrap();

        assert_eq!(options.get_username(), Some("user"));
        assert_eq!(options.get_password(), Some("password"));
        assert_eq!(options.get_host(), "hostname");
        assert_eq!(options.get_port(), 8915);
        assert_eq!(options.get_database(), Some("database"));
        assert_eq!(options.get_application_name(), Some("sqlx"));
    }

    #[test]
    fn parse_with_defaults() {
        let url = "postgres://";
        let options: PgConnectOptions = url.parse().unwrap();

        assert_eq!(options.get_username(), None);
        assert_eq!(options.get_password(), None);
        assert_eq!(options.get_host(), "localhost");
        assert_eq!(options.get_port(), 5432);
        assert_eq!(options.get_database(), None);
        assert_eq!(options.get_application_name(), None);
    }

    #[test]
    fn parse_socket_from_query() {
        let url = "postgresql://user:password@localhost/database?socket=/var/run/postgresql.sock";
        let options: PgConnectOptions = url.parse().unwrap();

        assert_eq!(options.get_username(), Some("user"));
        assert_eq!(options.get_password(), Some("password"));
        assert_eq!(options.get_database(), Some("database"));
        assert_eq!(options.get_socket(), Some(Path::new("/var/run/postgresql.sock")));
    }

    #[test]
    fn parse_socket_from_host() {
        // socket path in host requires URL encoding - but does work
        let url = "postgres://user:password@%2Fvar%2Frun%2Fpostgres%2Fpostgres.sock/database";
        let options: PgConnectOptions = url.parse().unwrap();

        assert_eq!(options.get_username(), Some("user"));
        assert_eq!(options.get_password(), Some("password"));
        assert_eq!(options.get_database(), Some("database"));
        assert_eq!(options.get_socket(), Some(Path::new("/var/run/postgres/postgres.sock")));
    }

    #[test]
    #[should_panic]
    fn fail_to_parse_non_postgres() {
        let url = "mysql://user:password@hostname:5432/database?timezone=system&charset=utf8";
        let _: PgConnectOptions = url.parse().unwrap();
    }

    #[test]
    fn parse_username_with_at_sign() {
        let url = "postgres://user@hostname:password@hostname:5432/database";
        let options: PgConnectOptions = url.parse().unwrap();

        assert_eq!(options.get_username(), Some("user@hostname"));
    }

    #[test]
    fn parse_password_with_non_ascii_chars() {
        let url = "postgres://username:p@ssw0rd@hostname:5432/database";
        let options: PgConnectOptions = url.parse().unwrap();

        assert_eq!(options.get_password(), Some("p@ssw0rd"));
    }
}
