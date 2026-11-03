//! The control protocol: short messages between nodes, next to file transfers.
//!
//! Every request opens its own QUIC stream: one message each way. The end of
//! the stream marks the end of the message, so no length prefix is needed.

use std::{sync::Arc, time::Duration};

use iroh::{
    endpoint::{Connection, ReadError, ReadToEndError, RecvStream, SendStream, VarInt},
    protocol::{AcceptError, ProtocolHandler},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Notify;
use tracing::{debug, warn};

use super::access::Access;
use crate::{
    contacts::{Contact, InviteToken, check_name, now_ms},
    error::Error,
    ids::{ContentHash, PeerId},
    safe_name,
    shares::{Offer, OfferState, PREVIEW_LEN},
};

/// Protocol name sent when connecting. The `1` is the version: an incompatible
/// change gets a new name, so old and new versions can run side by side.
pub(crate) const CONTROL_ALPN: &[u8] = b"caraxyn/control/1";

/// Largest message accepted.
const MAX_MSG: usize = 1 << 20;

/// How long the other side may take to send its whole message.
// shortcut: no test covers it (one would wait 10 s); make it a parameter if it needs one.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Stream error code sent when a message is refused: retrying will not help.
const REFUSED: VarInt = VarInt::from_u32(1);

/// Stream error code sent when the message could not be handled right now
/// (our own database failed): the sender should try again later.
const TRY_LATER: VarInt = VarInt::from_u32(2);

/// A control message. Only append new variants: postcard identifies them by position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ControlMsg {
    /// "You invited me": the token from the invite and the sender's name.
    ContactRequest { token: InviteToken, name: String },
    /// The invite was accepted. The inviter's name is already in the code.
    ContactAccepted,
    /// The invite was refused, unknown, used or expired.
    ContactRejected,
    /// "Here is a share for you": what the receiver's inbox shows.
    ShareOffer {
        root: ContentHash,
        title: String,
        file_count: u64,
        total_size: u64,
        /// The first files (at most [`PREVIEW_LEN`]) by path and size.
        preview: Vec<(String, u64)>,
        /// When the sender stops serving it, in unix ms, if ever.
        expires_at: Option<u64>,
    },
    /// The offer is in the receiver's inbox.
    OfferReceived,
    /// "I withdrew that share." Only acted on from the one who offered it.
    Revoke { root: ContentHash },
    /// Done; nothing more to say.
    Ack,
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
    /// The other node read the message and said no (not a contact, invalid…).
    #[error("refused by the other node")]
    Refused,
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
            // Told apart from a network failure: retrying a refusal never helps.
            ReadToEndError::Read(ReadError::Reset(code)) if code == REFUSED => {
                ControlError::Refused
            }
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
pub(crate) struct ControlHandler {
    pub(super) access: Arc<Access>,
    /// Woken when a peer with offers waiting for it shows up.
    pub(super) postman: Arc<Notify>,
}

impl ProtocolHandler for ControlHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        // Proven by the connection's encryption, so it cannot be faked.
        let peer = PeerId::from(conn.remote_id());
        // A peer that connects is online: send what waits for it now, not at the next retry.
        match self.access.db().offer_due_now(peer) {
            Ok(true) => self.postman.notify_one(),
            Ok(false) => {}
            Err(e) => warn!(peer = %peer.short(), %e, "could not check the outbox"),
        }
        // One stream per request, until the other side closes the connection.
        while let Ok((mut tx, mut rx)) = conn.accept_bi().await {
            let access = Arc::clone(&self.access);
            // Own task per request, so one slow stream does not hold up the others.
            tokio::spawn(async move {
                let answer = match recv(&mut rx).await {
                    Ok(msg) => answer(&access, peer, msg),
                    Err(e) => {
                        debug!(peer = %peer.short(), %e, "bad control message");
                        Err(REFUSED)
                    }
                };
                match answer {
                    Ok(answer) => {
                        if let Err(e) = send(&mut tx, &answer).await {
                            debug!(peer = %peer.short(), %e, "could not answer");
                        }
                    }
                    // Fails only this stream; the connection keeps working.
                    Err(code) => drop(tx.reset(code)),
                }
            });
        }
        Ok(())
    }
}

/// The answer to a request from `peer`, or the code to cancel its stream with.
fn answer(access: &Access, peer: PeerId, msg: ControlMsg) -> Result<ControlMsg, VarInt> {
    match msg {
        ControlMsg::ContactRequest { token, name } => {
            // Never says why: a stranger must not learn which tokens exist.
            let accepted = check_name(&name).is_ok() && {
                let contact = Contact {
                    peer,
                    name,
                    added_at: now_ms(),
                };
                access
                    .redeem(token, &contact, contact.added_at)
                    .unwrap_or_else(|e| {
                        warn!(peer = %peer.short(), %e, "could not check an invite");
                        false
                    })
            };
            Ok(if accepted {
                ControlMsg::ContactAccepted
            } else {
                ControlMsg::ContactRejected
            })
        }
        ControlMsg::ShareOffer {
            root,
            title,
            file_count,
            total_size,
            preview,
            expires_at,
        } => {
            let offer = Offer {
                from: peer,
                root,
                title,
                file_count,
                total_size,
                preview,
                expires_at,
                received_at: now_ms(),
                state: OfferState::New,
            };
            // Our own trouble is not a refusal: the sender keeps the offer and retries.
            let try_later = |e: Error| {
                warn!(peer = %peer.short(), %e, "could not take an offer");
                TRY_LATER
            };
            // Only contacts may offer, so strangers cannot fill the inbox.
            if !offer_is_valid(&offer) || !access.db().is_contact(peer).map_err(try_later)? {
                return Err(REFUSED);
            }
            access.db().receive_offer(&offer).map_err(try_later)?;
            Ok(ControlMsg::OfferReceived)
        }
        // Answers are never valid requests.
        // Looked up under the sender's own id, so nobody can revoke someone else's offer.
        ControlMsg::Revoke { root } => match access.db().revoke_offer(peer, root) {
            Ok(true) => Ok(ControlMsg::Ack),
            // Never offered by them (or never reached us): nothing to withdraw.
            Ok(false) => Err(REFUSED),
            Err(e) => {
                warn!(peer = %peer.short(), %e, "could not mark an offer revoked");
                Err(TRY_LATER)
            }
        },
        ControlMsg::ContactAccepted
        | ControlMsg::ContactRejected
        | ControlMsg::OfferReceived
        | ControlMsg::Ack => Err(REFUSED),
    }
}

/// Untrusted input: the title and every previewed path must be names we could
/// save, and the preview no longer than an honest sender makes it.
fn offer_is_valid(offer: &Offer) -> bool {
    safe_name::check(&offer.title).is_ok()
        && offer.preview.len() <= PREVIEW_LEN
        && offer
            .preview
            .iter()
            .all(|(path, _)| super::check_name(path).is_ok())
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
            ControlMsg::ContactAccepted,
            ControlMsg::ContactRejected,
            ControlMsg::ShareOffer {
                root: ContentHash::from([1; 32]),
                title: "trip".into(),
                file_count: 1,
                total_size: 2,
                preview: vec![("trip/a".into(), 2)],
                expires_at: Some(3),
            },
            ControlMsg::OfferReceived,
            ControlMsg::Revoke {
                root: ContentHash::from([1; 32]),
            },
            ControlMsg::Ack,
        ] {
            let bytes = postcard::to_allocvec(&msg).unwrap();
            assert_eq!(postcard::from_bytes::<ControlMsg>(&bytes).unwrap(), msg);
        }
    }
}
