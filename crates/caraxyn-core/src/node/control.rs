//! The control protocol: short messages between nodes, next to file transfers.
//!
//! Every request opens its own QUIC stream: one message each way. The end of
//! the stream marks the end of the message, so no length prefix is needed.

use std::time::Duration;

use iroh::{
    endpoint::{Connection, ReadToEndError, RecvStream, SendStream, VarInt},
    protocol::{AcceptError, ProtocolHandler},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

use crate::{contacts::InviteToken, ids::PeerId};

/// Protocol name sent when connecting. The `1` is the version: an incompatible
/// change gets a new name, so old and new versions can run side by side.
pub(crate) const CONTROL_ALPN: &[u8] = b"caraxyn/control/1";

/// Largest message accepted.
const MAX_MSG: usize = 1 << 20;

/// How long the other side may take to send its whole message.
// shortcut: no test covers it (one would wait 10 s); make it a parameter if it needs one.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Stream error code sent when a message is refused.
const REFUSED: VarInt = VarInt::from_u32(1);

/// A control message. Only append new variants: postcard identifies them by position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::enum_variant_names,
    reason = "the share messages are added later"
)]
pub(crate) enum ControlMsg {
    /// "You invited me": the token from the invite and the sender's name.
    ContactRequest { token: InviteToken, name: String },
    /// The invite was accepted; the name of the one who accepted.
    ContactAccepted { name: String },
    /// The invite was refused, unknown, used or expired.
    ContactRejected,
}

/// Why a control message could not be sent or received.
#[derive(Debug, Error)]
pub(crate) enum ControlError {
    #[error("message larger than 1 MiB")]
    TooLarge,
    #[error("not a valid control message")]
    Decode,
    #[error("no complete message within 10 s")]
    TimedOut,
    #[error("stream failed: {0}")]
    Stream(String),
}

fn stream_err(e: impl std::fmt::Display) -> ControlError {
    ControlError::Stream(e.to_string())
}

/// Sends `msg` and closes the sending side, which marks the end of the message.
pub(crate) async fn send(stream: &mut SendStream, msg: &ControlMsg) -> Result<(), ControlError> {
    let bytes = postcard::to_allocvec(msg).expect("a control message always serializes");
    stream.write_all(&bytes).await.map_err(stream_err)?;
    stream.finish().map_err(stream_err)
}

/// Reads one whole message, at most 1 MiB, within 10 s.
pub(crate) async fn recv(stream: &mut RecvStream) -> Result<ControlMsg, ControlError> {
    let bytes = tokio::time::timeout(READ_TIMEOUT, stream.read_to_end(MAX_MSG))
        .await
        .map_err(|_| ControlError::TimedOut)?
        .map_err(|e| match e {
            ReadToEndError::TooLong => ControlError::TooLarge,
            e => stream_err(e),
        })?;
    let (msg, rest) = postcard::take_from_bytes(&bytes).map_err(|_| ControlError::Decode)?;
    if !rest.is_empty() {
        return Err(ControlError::Decode);
    }
    Ok(msg)
}

/// Answers control requests from other nodes, strangers included: someone
/// redeeming an invite is not a contact yet. Each message is checked instead.
#[derive(Clone, Debug)]
pub(crate) struct ControlHandler;

impl ProtocolHandler for ControlHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let peer = PeerId::from(conn.remote_id());
        // One stream per request, until the other side closes the connection.
        while let Ok((mut tx, mut rx)) = conn.accept_bi().await {
            // Own task per request, so one slow stream does not hold up the others.
            tokio::spawn(async move {
                let answer = match recv(&mut rx).await {
                    Ok(msg) => answer(msg),
                    Err(e) => {
                        debug!(peer = %peer.short(), %e, "bad control message");
                        None
                    }
                };
                match answer {
                    Some(answer) => {
                        if let Err(e) = send(&mut tx, &answer).await {
                            debug!(peer = %peer.short(), %e, "could not answer");
                        }
                    }
                    // Fails only this stream; the connection keeps working.
                    None => drop(tx.reset(REFUSED)),
                }
            });
        }
        Ok(())
    }
}

/// The answer to a request, or `None` to refuse it.
fn answer(msg: ControlMsg) -> Option<ControlMsg> {
    match msg {
        // shortcut: no invites are stored yet, so every request is refused; the contact flow (step C) decides here.
        ControlMsg::ContactRequest { .. } => Some(ControlMsg::ContactRejected),
        // Answers are never valid requests.
        ControlMsg::ContactAccepted { .. } | ControlMsg::ContactRejected => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_survives_encoding() {
        for msg in [
            ControlMsg::ContactRequest {
                token: InviteToken::random(),
                name: "Jay".into(),
            },
            ControlMsg::ContactAccepted { name: "Sam".into() },
            ControlMsg::ContactRejected,
        ] {
            let bytes = postcard::to_allocvec(&msg).unwrap();
            assert_eq!(postcard::from_bytes::<ControlMsg>(&bytes).unwrap(), msg);
        }
    }
}
