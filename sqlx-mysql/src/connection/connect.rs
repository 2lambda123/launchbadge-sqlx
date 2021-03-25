//! Implements the connection phase.
//!
//! The connection phase (establish) performs these tasks:
//!
//! -   exchange the capabilities of client and server
//! -   setup SSL communication channel if requested
//! -   authenticate the client against the server
//!
//! The server may immediately send an ERR packet and finish the handshake
//! or send a `Handshake`.
//!
//! https://dev.mysql.com/doc/internals/en/connection-phase.html
//!
use sqlx_core::net::Stream as NetStream;
use sqlx_core::{Result, Runtime};

use crate::protocol::{AuthResponse, Capabilities, Handshake, HandshakeResponse};
use crate::{MySqlConnectOptions, MySqlConnection};

impl<Rt: Runtime> MySqlConnection<Rt> {
    fn handle_handshake(
        &mut self,
        options: &MySqlConnectOptions,
        handshake: &Handshake,
    ) -> Result<()> {
        // IF the options specify a database, try to use the CONNECT_WITH_DB capability
        // this lets us skip a round-trip after connect
        self.capabilities |= Capabilities::CONNECT_WITH_DB;

        // & the declared server capabilities with our capabilities to find
        // what rules the client should operate under
        self.capabilities &= handshake.capabilities;

        // store the connection ID, mainly for debugging
        self.connection_id = handshake.connection_id;

        // create the initial auth response
        // this may just be a request for an RSA public key
        let initial_auth_response = handshake
            .auth_plugin
            .invoke(&handshake.auth_plugin_data, options.get_password().unwrap_or_default());

        // the <HandshakeResponse> contains an initial guess at the correct encoding of
        // the password and some other metadata like "which database", "which user", etc.
        self.stream.write_packet(&HandshakeResponse {
            capabilities: self.capabilities,
            auth_plugin_name: handshake.auth_plugin.name(),
            auth_response: initial_auth_response,
            charset: 45, // [utf8mb4]
            database: options.get_database(),
            max_packet_size: 1024,
            username: options.get_username(),
        })?;

        Ok(())
    }

    fn handle_auth_response(
        &mut self,
        options: &MySqlConnectOptions,
        handshake: &mut Handshake,
        response: AuthResponse,
    ) -> Result<bool> {
        match response {
            AuthResponse::End(res) => {
                let _ok = res.into_result()?;

                // successful, simple authentication; good to go
                return Ok(true);
            }

            AuthResponse::Command(command, data) => {
                if let Some(data) = handshake.auth_plugin.handle(
                    command,
                    data,
                    &handshake.auth_plugin_data,
                    options.get_password().unwrap_or_default(),
                )? {
                    // write the response from the plugin
                    self.stream.write_packet(&&*data)?;
                }
            }

            AuthResponse::Switch(sw) => {
                // switch to the new plugin
                handshake.auth_plugin = sw.plugin;
                handshake.auth_plugin_data = sw.plugin_data;

                // generate an initial response from this plugin
                let data = handshake.auth_plugin.invoke(
                    &handshake.auth_plugin_data,
                    options.get_password().unwrap_or_default(),
                );

                // write the response from the plugin
                self.stream.write_packet(&&*data)?;
            }
        }

        Ok(false)
    }
}

macro_rules! impl_connect {
    (@blocking @new $options:ident) => {
        NetStream::connect($options.address.as_ref())?
    };

    (@new $options:ident) => {
        NetStream::connect_async($options.address.as_ref()).await?
    };

    ($(@$blocking:ident)? $options:ident) => {{
        // open a network stream to the database server
        let stream = impl_connect!($(@$blocking)? @new $options);

        // construct a <MySqlConnection> around the network stream
        // wraps the stream in a <BufStream> to buffer read and write
        let mut self_ = Self::new(stream);

        // immediately the server should emit a <Handshake> packet
        // we need to handle that and reply with a <HandshakeResponse>
        let mut handshake = read_packet!($(@$blocking)? self_.stream).deserialize()?;
        self_.handle_handshake($options, &handshake)?;

        loop {
            let response = read_packet!($(@$blocking)? self_.stream).deserialize_with(self_.capabilities)?;
            if self_.handle_auth_response($options, &mut handshake, response)? {
                // complete, successful authentication
                break;
            }
        }

        Ok(self_)
    }};
}

impl<Rt: Runtime> MySqlConnection<Rt> {
    #[cfg(feature = "async")]
    pub(crate) async fn connect_async(options: &MySqlConnectOptions) -> Result<Self>
    where
        Rt: sqlx_core::Async,
    {
        impl_connect!(options)
    }

    #[cfg(feature = "blocking")]
    pub(crate) fn connect_blocking(options: &MySqlConnectOptions) -> Result<Self>
    where
        Rt: sqlx_core::blocking::Runtime,
    {
        impl_connect!(@blocking options)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use sqlx_core::blocking::ConnectOptions;
    use sqlx_core::mock::Mock;

    use crate::mock::MySqlMockStreamExt;
    use crate::{MySqlConnectOptions, MySqlConnection};

    const MOCK_DATABASE_URL: &str = "mysql://root:password@localhost/";

    #[test]
    fn should_connect_default_native_auth() -> anyhow::Result<()> {
        let mut mock = Mock::stream();

        // Handshake
        mock.write_packet(0, b"\n5.5.5-10.5.8-MariaDB-1:10.5.8+maria~focal\0)\0\0\04bo+$r4H\0\xfe\xf7-\x02\0\xff\x81\x15\0\0\0\0\0\0\x0f\0\0\0O5X>j}Ur]Y)^\0mysql_native_password\0")?;

        // AuthResponse::End > ResultPacket::Ok
        mock.write_packet(2, b"\0\0\0\x02\0\0\0")?;

        let _conn: MySqlConnection<Mock> =
            MySqlConnectOptions::from_str(MOCK_DATABASE_URL)?.port(mock.port()).connect()?;

        // HandshakeResponse
        mock.expect_packet(b"Q\0\0\x01\x0c\xa3\xef\x01\0\x04\0\0-\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0root\0\x14P\xaf\xf1\x12,\xe9\xad\xea\x7f\xa0\n\xcd\xa2\xb5<\x17\xa5\xc9J\xd0\0mysql_native_password\0")?;

        assert!(mock.is_empty());

        Ok(())
    }

    #[test]
    fn should_connect_default_sha256_auth() -> anyhow::Result<()> {
        let mut mock = Mock::stream();

        // Handshake
        mock.write_packet(0, b"\n8.0.22\0\x0e\0\0\0\x1b\x02O\x04hL8D\0\xff\xff\xff\x02\0\xff\xc7\x15\0\0\0\0\0\0\0\0\0\0^*Nh\x19\x1f*)-\x0c\x07v\0sha256_password\0")?;

        // AuthResponse::Command(0x01, AUTH_CONTINUE)
        mock.write_packet(2, b"\x01-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAwnXi3nr9TmN+NF49A3Y7\nUBnAVhApNJy2cmuf/y6vFM9eHFu5T80Ij1qYc6c79oAGA8nNNCFQL+0j5De88cln\nKrlzq/Ab3U+j5SqgNwk//F6Y3iyjV4L7feSDqjpcheFzkjEslbm/yoRwQ78AAU6s\nqA0hcFuh66mcvnotDrvZAGQ8U2EbbZa6oiR3wrgbzifSKq767g65zIrCpoyxzKMH\nAETSDIaMKpFio4dRATKT5ASQtPoIyxSBmjRtc22sqlhEeiejEMsJzd6Bliuait+A\nkTXL6G1Tbam26Dok/L88CnTAWAkLwTA3bjPcS8Zl9gTsJvoiMuwW1UPEVV/aJ11Z\n/wIDAQAB\n-----END PUBLIC KEY-----\n")?;

        // AuthResponse::End > ResultPacket::Ok
        mock.write_packet(4, b"\0\0\0\x02\0\0\0")?;

        let _conn: MySqlConnection<Mock> =
            MySqlConnectOptions::from_str(MOCK_DATABASE_URL)?.port(mock.port()).connect()?;

        // HandshakeResponse
        mock.expect_packet(b"8\0\0\x01\r\xa3\xef\x01\0\x04\0\0-\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0root\0\x01\x01\0sha256_password\0")?;

        // encoded password after receiving RSA public key
        mock.expect_packet(b"\0\x01\0\x03\xc1*\xf5=\xc3\x86\x95U$=\x9c \x946_Rg\xdc\x9d\xa0M\xf2@\xba\xf7\x8f\rE\xdbrI\xac\x05\xfb\xd1\xaa\r0 '\xf2\xec\xb3Xu\x98\x82\xf2\x8d)\x80\xe7\xdcG\\\xde\x87\x0e\x07\x87f\xach\xbb\x0b\xdf\xe0\xd9\xd1N\x9f_\x17xT\xec\xd5\xff\xd3\xa35\x11PO\xca\xf2\x13?=n\xe7\xd5\xbb\xa0\xd0\xca\xc5\x80\xb0\0\xc0\xe9F\x90f\xa0a\xd1\xdb\xe4(\xed2\xd7@\xb8u\x859U\xd6\xa2\xc3\xa2\xbe\x9a\xeeSy\x92\x95\r\xd3\x14\x90\x80\xb1o#\xa6\x7f\x16\x7f\t-'\xf35\xa02zY\xaeP^e\xf9O\xed\x9d\xb5\x8b\x9d\x0cayA\xff\"-\x80\x8c<\xc4\x11e\xdf\x9c\xe2\x9b)\x8f\xb0\xe9\xe1\xbcj\xf9\xa0U\xe6\x95\x9b\x01 \xba\x7f\"\\\x0cF9\\'\xf2\xfcMD\x1a\xd8\xe3\x11\xdfN\xc4\xd3\x9e\xee\x8d\r\xda\x94\xc4\xafR\xf3\x1e8b\x8d$\x84Nj\x18~\xa7\xf1\x8bb&\x90\xc0\xad\xb1O\xec\xfa\x98h\xf0{.\x07R\n")?;

        assert!(mock.is_empty());

        Ok(())
    }

    #[test]
    fn should_connect_default_caching_sha2_auth() -> anyhow::Result<()> {
        let mut mock = Mock::stream();

        // Handshake
        mock.write_packet(0, b"\n8.0.22\0\x08\0\0\0TIbl}%U#\0\xff\xff\xff\x02\0\xff\xc7\x15\0\0\0\0\0\0\0\0\0\0\x06\x12\x0e`5\x1b\x12\x0b\x13\x06_\x19\0caching_sha2_password\0")?;

        // AuthResponse::Command(0x01, AUTH_CONTINUE)
        mock.write_packet(2, b"\x01\x04")?;

        // AuthResponse::Command(0x01, ..)
        mock.write_packet(4, b"\x01-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAwnXi3nr9TmN+NF49A3Y7\nUBnAVhApNJy2cmuf/y6vFM9eHFu5T80Ij1qYc6c79oAGA8nNNCFQL+0j5De88cln\nKrlzq/Ab3U+j5SqgNwk//F6Y3iyjV4L7feSDqjpcheFzkjEslbm/yoRwQ78AAU6s\nqA0hcFuh66mcvnotDrvZAGQ8U2EbbZa6oiR3wrgbzifSKq767g65zIrCpoyxzKMH\nAETSDIaMKpFio4dRATKT5ASQtPoIyxSBmjRtc22sqlhEeiejEMsJzd6Bliuait+A\nkTXL6G1Tbam26Dok/L88CnTAWAkLwTA3bjPcS8Zl9gTsJvoiMuwW1UPEVV/aJ11Z\n/wIDAQAB\n-----END PUBLIC KEY-----\n")?;

        // AuthResponse::End > ResultPacket::Ok
        mock.write_packet(6, b"\0\0\0\x02\0\0\0")?;

        let _conn: MySqlConnection<Mock> =
            MySqlConnectOptions::from_str(MOCK_DATABASE_URL)?.port(mock.port()).connect()?;

        // HandshakeResponse
        mock.expect_packet(b"]\0\0\x01\r\xa3\xef\x01\0\x04\0\0-\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0root\0 \x9d\x85T\x15\xfe\xa9u\x13\x02&\x9dlG\x17\x98\x1b`\x8a\x96\xfcI\x19\x17\xe0(I8\xba\xd7\xfax\xa9\0caching_sha2_password\0")?;

        // ask for RSA key
        mock.expect_packet(b"\x01\0\0\x03\x02")?;

        // encoded password after receiving RSA public key
        mock.expect_packet(b"\0\x01\0\x05#7\x8f\xd6\x8dCi9*\xee\x87\xb3\xb1,@\xdf\x94\xa8g\xbf\xed5\xf3\x1e\x9c\xfe\xda\xe8-6\x9c\x1eO\xb6\x80\x81]h\x0b\xd8\x10xx\xeb\x8b\xe9\x8a\x93\xd7\x83\xf7\x9a\xe1\xb94\xfd\xb0\x81\xeb\x0f\xecU:\xf4\x82\x11\xd3\xee\x8e+\x9e_rm\xb4\xbdM\xa0\x90\xff\xc3\x03V*\xa6|\x16\xdd\xea\xd2\x92\xef\xf5E\xb1t\n\xb7\xd9\x8bU\xbd\x94\xb8\x80|S+z\x1bO\x1e\xdf&\xf7(\xf0~\x97\x8b\xee1\xa4\xbb\x9f6\xc4\x88\xbf\x14$\xb2\xc0\xea\x9f\xdd\xfc\x99\xc8\xfe\x178\xf3X\x90\x01\xcc\xa8\x86\x9d\xe9\x98\xbf\xc2\xdc\xe8\xff\x96\xbd^\xf6\r \xb5\xe8\x0euo\xb5(\x80\xffW7\xf0\xdd\xcc\xaa\x9fYl\xef\xb7y\xf7A\xf4\xcf\x1f\xfc\rS\x7f\x13\xa9b\xadd\x1c\xcf\xf5\x98\x0ei\xc3\x0f\x9c\x8eqeTu\x8b\x17\xe7\xd47\xc5\xe9j=\xfc\x82\x04\x96}V.U?\x85\x14J\xe2\xd3.+:\xc5\xe0'm\x9a3\x85\x1e\xf7\xad\xf9J\xcf\xfc\xa7\xc2\x04@")?;

        assert!(mock.is_empty());

        Ok(())
    }

    #[test]
    fn should_reconnect_default_caching_sha2_auth() -> anyhow::Result<()> {
        let mut mock = Mock::stream();

        // Handshake
        mock.write_packet(0, b"\n8.0.22\0\x08\0\0\0TIbl}%U#\0\xff\xff\xff\x02\0\xff\xc7\x15\0\0\0\0\0\0\0\0\0\0\x06\x12\x0e`5\x1b\x12\x0b\x13\x06_\x19\0caching_sha2_password\0")?;

        // AuthResponse::Command(0x01, AUTH_OK)
        mock.write_packet(2, b"\x01\x03")?;

        // AuthResponse::End > ResultPacket::Ok
        mock.write_packet(4, b"\0\0\0\x02\0\0\0")?;

        let _conn: MySqlConnection<Mock> =
            MySqlConnectOptions::from_str(MOCK_DATABASE_URL)?.port(mock.port()).connect()?;

        // HandshakeResponse
        mock.expect_packet(b"]\0\0\x01\r\xa3\xef\x01\0\x04\0\0-\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0root\0 \x9d\x85T\x15\xfe\xa9u\x13\x02&\x9dlG\x17\x98\x1b`\x8a\x96\xfcI\x19\x17\xe0(I8\xba\xd7\xfax\xa9\0caching_sha2_password\0")?;

        assert!(mock.is_empty());

        Ok(())
    }

    #[test]
    fn should_connect_switch_native_auth() -> anyhow::Result<()> {
        let mut mock = Mock::stream();

        // Handshake
        mock.write_packet(0, b"\n8.0.22\0\x08\0\0\0TIbl}%U#\0\xff\xff\xff\x02\0\xff\xc7\x15\0\0\0\0\0\0\0\0\0\0\x06\x12\x0e`5\x1b\x12\x0b\x13\x06_\x19\0caching_sha2_password\0")?;

        // AuthResponse::Switch
        mock.write_packet(2, b"\xfemysql_native_password\0\r.89j]CpA3Ov~\x1de\\/\x15,\r\0")?;

        // AuthResponse::End > ResultPacket::Ok
        mock.write_packet(4, b"\0\0\0\x02\0\0\0")?;

        let _conn: MySqlConnection<Mock> =
            MySqlConnectOptions::from_str(MOCK_DATABASE_URL)?.port(mock.port()).connect()?;

        // HandshakeResponse
        mock.expect_packet(b"]\0\0\x01\r\xa3\xef\x01\0\x04\0\0-\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0root\0 \x9d\x85T\x15\xfe\xa9u\x13\x02&\x9dlG\x17\x98\x1b`\x8a\x96\xfcI\x19\x17\xe0(I8\xba\xd7\xfax\xa9\0caching_sha2_password\0")?;

        // scrambled password after switch
        mock.expect_packet(b"\x14\0\0\x031.Z\x95JON\x81\x9ak\xc7\xba\xe6{L\x0f\xe8\x03N\xef")?;

        assert!(mock.is_empty());

        Ok(())
    }

    #[test]
    fn should_connect_switch_caching_sha2_auth() -> anyhow::Result<()> {
        let mut mock = Mock::stream();

        // Handshake
        mock.write_packet(0, b"\n5.5.5-10.5.9-MariaDB-1:10.5.9+maria~focal\0\x03\0\0\0]?bRT-\"`\0\xfe\xf7-\x02\0\xff\x81\x15\0\0\0\0\0\0\x0f\0\0\0Bhc:7D/^k#f[\0mysql_native_password\0")?;

        // AuthResponse::Switch
        mock.write_packet(2, b"\xfecaching_sha2_password\0\x12}Wz?0-M9sO*S\x03\nP\x1c]pe\0")?;

        // AuthResponse::Command(0x1, AUTH_CONTINUE)
        mock.write_packet(4, b"\x01\x04")?;

        // AuthResponse::Command(0x1, ..)
        mock.write_packet(6, b"\x01-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAwnXi3nr9TmN+NF49A3Y7\nUBnAVhApNJy2cmuf/y6vFM9eHFu5T80Ij1qYc6c79oAGA8nNNCFQL+0j5De88cln\nKrlzq/Ab3U+j5SqgNwk//F6Y3iyjV4L7feSDqjpcheFzkjEslbm/yoRwQ78AAU6s\nqA0hcFuh66mcvnotDrvZAGQ8U2EbbZa6oiR3wrgbzifSKq767g65zIrCpoyxzKMH\nAETSDIaMKpFio4dRATKT5ASQtPoIyxSBmjRtc22sqlhEeiejEMsJzd6Bliuait+A\nkTXL6G1Tbam26Dok/L88CnTAWAkLwTA3bjPcS8Zl9gTsJvoiMuwW1UPEVV/aJ11Z\n/wIDAQAB\n-----END PUBLIC KEY-----\n")?;

        // AuthResponse::End -> ResultPacket::Ok
        mock.write_packet(8, b"\0\0\0\x02\0\0\0")?;

        let _conn: MySqlConnection<Mock> =
            MySqlConnectOptions::from_str(MOCK_DATABASE_URL)?.port(mock.port()).connect()?;

        // HandshakeResponse
        mock.expect_packet(b"Q\0\0\x01\x0c\xa3\xef\x01\0\x04\0\0-\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0root\0\x14\xe0\xbe\xa9\x93\xab\x17.@_\xf1t\xd9\xe6_\xfcgfV\xf6\x85\0mysql_native_password\0")?;

        // sha2 scramble of password
        mock.expect_packet(b" \0\0\x03\xffjg\x06p\x1d\xeawto\xf3\xf6\xa0\x9f7\xa9Z\xb3\xa5\xf9\x0b\x80\x14j8WTb\xf1{f\xf5")?;

        // ask for RSA public key
        mock.expect_packet(b"\x01\0\0\x05\x02")?;

        // encoded password after receiving RSA public key
        mock.expect_packet(b"\0\x01\0\x077fS:\x9d3\xec\xe47\xbe\xda\xd8a\x14\x7f\xa8\xa82\x15\xb3\xb8\xa4D\x8f\x8e,,\xc4\x7f\x9ck\x9cI2&\xc2a\xd4\xef\r\x04\xc2\xd1\x89\xb05\xab\xe2YL\xd2hz\xf6y\xb7\xcb\x08\x9a\x1d\xc0A\x7f\x97\xba*\x1e,c\xbcP\xab\xa2\xee\xfa\xcd^=\x1flj\x96\x8fGx\x8e\x9b\xfd\xea\xd05w\xcc\xf2\xfc\xf8\xb4Pm;\xc4\x94}A~=R\xbcr\xbb?\xd1]\r\xb1\xd9{\xf6\x1b%\x14iAe\x04a\x91\x144q\x1e\x92H\xcb\xe7z,+1!6#\x92\x8c\x12o\x8eyb\xe7g\xd2[\x11W\xfeJ\xe3.\x88C\x1a$\xa5\xfa\xfd\xe1\x1e\x0c4\xc5\xbf7\x94\xca$\x0c\xa6\xbc\x07d\x04\x0f\xe4\xfc\xbeZ\x1c7\xce\x0c^8@d; \xf9\xfe\x1dU\x15\x9e\x9f[b\xe6Z\xda\xa9\x17\xcf\xd9\xa8\x0b\x10\xf5\xe3\xa1\xc0\xe2Z\x8b\x9fq\xe9\xe8\x97f\x1bY\xec\xbc\x8b\x89\x9a\xeb\xffU\xe2\xfa#%\xa5d\xfa\xeb\x15\"\x8a\xf4R\x85\xdf\xe3\xcd")?;

        assert!(mock.is_empty());

        Ok(())
    }

    #[test]
    fn should_reconnect_switch_caching_sha2_auth() -> anyhow::Result<()> {
        let mut mock = Mock::stream();

        // Handshake
        mock.write_packet(0, b"\n5.5.5-10.5.9-MariaDB-1:10.5.9+maria~focal\0\x03\0\0\0]?bRT-\"`\0\xfe\xf7-\x02\0\xff\x81\x15\0\0\0\0\0\0\x0f\0\0\0Bhc:7D/^k#f[\0mysql_native_password\0")?;

        // AuthResponse::Switch
        mock.write_packet(2, b"\xfecaching_sha2_password\0\x12}Wz?0-M9sO*S\x03\nP\x1c]pe\0")?;

        // AuthResponse::Command(0x1, AUTH_OK)
        mock.write_packet(4, b"\x01\x03")?;

        // AuthResponse::End -> ResultPacket::Ok
        mock.write_packet(6, b"\0\0\0\x02\0\0\0")?;

        let _conn: MySqlConnection<Mock> =
            MySqlConnectOptions::from_str(MOCK_DATABASE_URL)?.port(mock.port()).connect()?;

        // HandshakeResponse
        mock.expect_packet(b"Q\0\0\x01\x0c\xa3\xef\x01\0\x04\0\0-\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0root\0\x14\xe0\xbe\xa9\x93\xab\x17.@_\xf1t\xd9\xe6_\xfcgfV\xf6\x85\0mysql_native_password\0")?;

        // sha2 scramble of password
        mock.expect_packet(b" \0\0\x03\xffjg\x06p\x1d\xeawto\xf3\xf6\xa0\x9f7\xa9Z\xb3\xa5\xf9\x0b\x80\x14j8WTb\xf1{f\xf5")?;

        assert!(mock.is_empty());

        Ok(())
    }

    #[test]
    fn should_connect_empty_auth() -> anyhow::Result<()> {
        let mut mock = Mock::stream();

        // Handshake
        mock.write_packet(0, b"\n5.5.5-10.5.9-MariaDB-1:10.5.9+maria~focal\0\x03\0\0\0]?bRT-\"`\0\xfe\xf7-\x02\0\xff\x81\x15\0\0\0\0\0\0\x0f\0\0\0Bhc:7D/^k#f[\0mysql_native_password\0")?;

        // AuthResponse::End -> ResultPacket::Ok
        mock.write_packet(2, b"\0\0\0\x02\0\0\0")?;

        let _conn: MySqlConnection<Mock> =
            MySqlConnectOptions::new().port(mock.port()).username("root").connect()?;

        // HandshakeResponse
        mock.expect_packet(b"=\0\0\x01\x0c\xa3\xef\x01\0\x04\0\0-\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0root\0\0\0mysql_native_password\0")?;

        assert!(mock.is_empty());

        Ok(())
    }

    #[test]
    fn should_fail_connect_err() -> anyhow::Result<()> {
        let mut mock = Mock::stream();

        // Handshake
        mock.write_packet(0, b"\n5.5.5-10.5.9-MariaDB-1:10.5.9+maria~focal\0\x03\0\0\0]?bRT-\"`\0\xfe\xf7-\x02\0\xff\x81\x15\0\0\0\0\0\0\x0f\0\0\0Bhc:7D/^k#f[\0mysql_native_password\0")?;

        // AuthResponse::End -> ResultPacket::Err
        mock.write_packet(
            2,
            b"\xff\x15\x04#28000Access denied for user 'root'@'172.17.0.1' (using password: YES)",
        )?;

        let err = MySqlConnectOptions::new()
            .port(mock.port())
            .username("root")
            .connect::<MySqlConnection<Mock>, _>()
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "1045 (28000): Access denied for user \'root\'@\'172.17.0.1\' (using password: YES)"
        );

        Ok(())
    }

    #[test]
    fn should_fail_interactive_connect_with_dialog() -> anyhow::Result<()> {
        let mut mock = Mock::stream();

        // Handshake
        mock.write_packet(0, b"\n5.5.5-10.5.9-MariaDB\0+\0\0\07y\"7/$dN\0\xfe\xf7\xe0\x02\0\xff\x81\x15\0\0\0\0\0\0\x0f\0\0\0<~(Iv7Nc(9)`\0mysql_native_password\0")?;

        // AuthResponse::Switch
        mock.write_packet(2, b"\xfedialog\0")?;

        // AuthResponse::Command(0x4, ..)
        mock.write_packet(4, b"\x04Password: ")?;

        let err = MySqlConnectOptions::new()
            .port(mock.port())
            .username("root")
            .connect::<MySqlConnection<Mock>, _>()
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "2061 (HY000): Authentication plugin \'dialog\' reported error: interactive dialog authentication is currently not supported"
        );

        Ok(())
    }

    #[test]
    fn should_not_connect_old_auth() -> anyhow::Result<()> {
        let mut mock = Mock::stream();

        mock.write_packet(0, b"\n5.5.5-10.5.8-MariaDB-1:10.5.8+maria~focal\0)\0\0\04bo+$r4H\0\xfe\xf7-\x02\0\xff\x81\x15\0\0\0\0\0\0\x0f\0\0\0O5X>j}Ur]Y)^\0mysql_old_password\0")?;

        let err = MySqlConnectOptions::new()
            .port(mock.port())
            .username("root")
            .password("password")
            .connect::<MySqlConnection<Mock>, _>()
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "2059 (HY000): Authentication plugin 'mysql_old_password' cannot be loaded"
        );

        Ok(())
    }
}
