//! The outbox: offers wait here until their recipient can be reached.
//!
//! A background task, the postman, sends whatever is due, all at once. It
//! retries with growing pauses, and is woken early at startup, by a new share
//! and when a recipient connects to us.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::watch;
use tracing::{info, warn};

use super::{
    Node, Timing,
    control::{ControlError, ControlMsg},
    db::OutboxEntry,
};
use crate::{
    contacts::now_ms,
    error::Error,
    ids::{ContentHash, PeerId},
    shares::PendingOffer,
};

/// What came of one delivery attempt.
enum Delivery {
    /// The offer is in the recipient's inbox.
    Delivered,
    /// The recipient said no, or the share no longer exists: never retry.
    Dropped,
    /// Not reached (offline, network trouble): try again later.
    Failed,
}

/// Pause before the next try after `attempts` failures: the first pause,
/// doubled each time, never more than the longest.
pub(super) fn retry_delay(attempts: u32, timing: &Timing) -> Duration {
    let doublings = attempts.saturating_sub(1).min(16);
    timing
        .retry_first
        .saturating_mul(1 << doublings)
        .min(timing.retry_max)
}

impl Node {
    /// Every offer still waiting to reach its recipient.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] if the outbox cannot be read.
    pub fn pending_offers(&self) -> Result<Vec<PendingOffer>, Error> {
        Ok(self
            .access
            .db()
            .outbox()?
            .into_iter()
            .map(|entry| PendingOffer {
                to: entry.to,
                root: entry.root,
                attempts: entry.attempts,
            })
            .collect())
    }

    /// Runs until `stop` changes: starts sending what is due, then sleeps until
    /// the next offer is due or something wakes it (a new share, a recipient
    /// connecting, a delivery finishing).
    pub(super) async fn run_postman(self, mut stop: watch::Receiver<bool>) {
        // Offers being sent right now, so a slow one is never sent twice at once.
        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        // At startup every waiting offer is tried at once, whatever its schedule.
        let mut everything = true;
        loop {
            let wait = self.send_due(everything, &in_flight).unwrap_or_else(|e| {
                warn!(%e, "could not read the outbox");
                self.timing.retry_max
            });
            everything = false;
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = self.postman.notified() => {}
                _ = stop.changed() => return,
            }
        }
    }

    /// Starts sending every due offer that is not already on its way; each one
    /// records its own result, so a slow recipient holds up nobody else.
    /// Returns how long until the next waiting offer is due.
    fn send_due(
        &self,
        everything: bool,
        in_flight: &Arc<Mutex<HashSet<(PeerId, ContentHash)>>>,
    ) -> Result<Duration, Error> {
        let now = now_ms();
        let mut next: Option<u64> = None;
        for entry in self.access.db().outbox()? {
            let key = (entry.to, entry.root);
            if in_flight
                .lock()
                .expect("outbox lock poisoned")
                .contains(&key)
            {
                continue;
            }
            if !everything && entry.next_try > now {
                next = Some(next.map_or(entry.next_try, |n| n.min(entry.next_try)));
                continue;
            }
            in_flight.lock().expect("outbox lock poisoned").insert(key);
            let (node, in_flight) = (self.clone(), Arc::clone(in_flight));
            tokio::spawn(async move {
                let delivery = node.deliver(entry.to, entry.root).await;
                if let Err(e) = node.record(entry, delivery) {
                    warn!(%e, "could not update the outbox");
                }
                in_flight.lock().expect("outbox lock poisoned").remove(&key);
                // The postman recomputes when the next offer is due.
                node.postman.notify_one();
            });
        }
        Ok(next.map_or(self.timing.retry_max, |at| {
            Duration::from_millis(at.saturating_sub(now))
        }))
    }

    /// Updates the outbox after one attempt.
    fn record(&self, entry: OutboxEntry, delivery: Delivery) -> Result<(), Error> {
        let (to, root) = (entry.to, entry.root);
        match delivery {
            Delivery::Delivered => {
                info!(to = %to.short(), share = %root.short(), "offer delivered");
                self.access.db().remove_offer(to, root)
            }
            Delivery::Dropped => {
                warn!(to = %to.short(), share = %root.short(), "offer refused, not retrying");
                self.access.db().remove_offer(to, root)
            }
            Delivery::Failed => {
                let attempts = entry.attempts.saturating_add(1);
                let pause = retry_delay(attempts, &self.timing);
                let pause = u64::try_from(pause.as_millis()).unwrap_or(u64::MAX);
                let at = now_ms().saturating_add(pause);
                self.access.db().offer_failed(to, root, attempts, at)
            }
        }
    }

    /// One attempt to put the offer for `root` into `to`'s inbox.
    async fn deliver(&self, to: PeerId, root: ContentHash) -> Delivery {
        let share = match self.access.db().share(root) {
            Ok(Some(share)) => share,
            Ok(None) => return Delivery::Dropped,
            Err(_) => return Delivery::Failed,
        };
        // No longer a recipient (removed meanwhile): not even the title may reach them.
        if !share.recipients.contains(&to) {
            return Delivery::Dropped;
        }
        let offer = ControlMsg::ShareOffer {
            root,
            title: share.title,
            file_count: share.file_count,
            total_size: share.total_size,
            preview: share.preview,
        };
        let answer =
            tokio::time::timeout(self.timing.offer_timeout, self.request(to, &offer)).await;
        match answer {
            Ok(Ok(ControlMsg::OfferReceived)) => Delivery::Delivered,
            // Any other answer means it was read and not taken.
            Ok(Ok(_) | Err(ControlError::Refused)) => Delivery::Dropped,
            Ok(Err(_)) | Err(_) => Delivery::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::TIMING;

    #[test]
    fn pauses_double_from_30_seconds_up_to_10_minutes() {
        let secs = |attempts| retry_delay(attempts, &TIMING).as_secs();
        assert_eq!(
            [1, 2, 3, 4, 5, 6, 7, 100].map(secs),
            [30, 60, 120, 240, 480, 600, 600, 600]
        );
    }
}
