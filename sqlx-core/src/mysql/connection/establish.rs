use bytes::Bytes;
use hashbrown::HashMap;

use crate::error::Error;
use crate::mysql::connection::{tls, MySqlStream, COLLATE_UTF8MB4_UNICODE_CI, MAX_PACKET_SIZE};
use crate::mysql::protocol::connect::{
    AuthSwitchRequest, AuthSwitchResponse, Handshake, HandshakeResponse,
};
use crate::mysql::protocol::Capabilities;
use crate::mysql::{MySqlConnectOptions, MySqlConnection, MySqlSslMode};
use bytes::buf::BufExt;

impl MySqlConnection {
    pub(crate) async fn establish(options: &MySqlConnectOptions) -> Result<Self, Error> {
        let mut stream: MySqlStream = MySqlStream::connect(options).await?;

        // https://dev.mysql.com/doc/dev/mysql-server/8.0.12/page_protocol_connection_phase.html
        // https://mariadb.com/kb/en/connection/

        let handshake: Handshake = stream.recv_packet().await?.decode()?;

        let mut plugin = handshake.auth_plugin;
        let mut nonce = handshake.auth_plugin_data;

        stream.capabilities &= handshake.server_capabilities;
        stream.capabilities |= Capabilities::PROTOCOL_41;

        if matches!(options.ssl_mode, MySqlSslMode::Disabled) {
            // remove the SSL capability if SSL has been explicitly disabled
            stream.capabilities.remove(Capabilities::SSL);
        }

        // Upgrade to TLS if we were asked to and the server supports it
        tls::maybe_upgrade(&mut stream, options).await?;

        let auth_response = if let (Some(plugin), Some(password)) = (plugin, &options.password) {
            Some(plugin.scramble(&mut stream, password, &nonce).await?)
        } else {
            None
        };

        stream.write_packet(HandshakeResponse {
            char_set: COLLATE_UTF8MB4_UNICODE_CI,
            max_packet_size: MAX_PACKET_SIZE,
            username: &options.username,
            database: options.database.as_deref(),
            auth_plugin: plugin,
            auth_response: auth_response.as_deref(),
        });

        stream.flush().await?;

        loop {
            let packet = stream.recv_packet().await?;
            match packet[0] {
                0x00 => {
                    let _ok = packet.ok()?;

                    break;
                }

                0xfe => {
                    let switch: AuthSwitchRequest = packet.decode()?;

                    plugin = Some(switch.plugin);
                    nonce = switch.data.chain(Bytes::new());

                    let response = switch
                        .plugin
                        .scramble(
                            &mut stream,
                            options.password.as_deref().unwrap_or_default(),
                            &nonce,
                        )
                        .await?;

                    stream.write_packet(AuthSwitchResponse(response));
                    stream.flush().await?;
                }

                id => {
                    if let (Some(plugin), Some(password)) = (plugin, &options.password) {
                        if plugin.handle(&mut stream, packet, password, &nonce).await? {
                            // plugin signaled authentication is ok
                            break;
                        }

                    // plugin signaled to continue authentication
                    } else {
                        return Err(err_protocol!(
                            "unexpected packet 0x{:02x} during authentication",
                            id
                        ));
                    }
                }
            }
        }

        Ok(Self {
            stream,
            cache_statement: HashMap::new(),
            scratch_row_columns: Default::default(),
            scratch_row_column_names: Default::default(),
        })
    }
}
